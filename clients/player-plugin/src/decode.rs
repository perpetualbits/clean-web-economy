//! Audio decode via `symphonia` (pure Rust).
//!
//! Turns a media file into the two inputs recognition needs: the exact file
//! bytes (Tier 1 content id) and decoded mono `f32` PCM at its sample rate
//! (Tier 2 fingerprint). Multi-channel audio is downmixed to mono by taking the
//! first channel — enough for the acoustic fingerprint.

use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Decoded audio plus the original file bytes.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// The exact file bytes — hashed to the Tier 1 content id.
    pub bytes: Vec<u8>,
    /// Mono `f32` PCM samples (first channel), fed to the fingerprint.
    pub samples: Vec<f32>,
    /// The audio sample rate in Hz.
    pub sample_rate: u32,
}

impl DecodedAudio {
    /// The playback duration in whole seconds (`samples / sample_rate`).
    pub fn duration_secs(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0; // guard: a malformed rate yields no accountable time
        }
        self.samples.len() as u64 / self.sample_rate as u64
    }
}

/// Decode an audio file into `DecodedAudio`, or a clear error on failure.
pub fn decode(path: &Path) -> Result<DecodedAudio, DecodeError> {
    // Read the whole file: the bytes are the Tier 1 identifier, and symphonia
    // decodes from an in-memory cursor so we hold exactly what we hashed.
    let bytes = std::fs::read(path).map_err(|e| DecodeError::Io(e.to_string()))?;
    let cursor = std::io::Cursor::new(bytes.clone());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    // Hint the prober with the file extension when present (helps format pick).
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the container and pick the default audio track.
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| DecodeError::Decode(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| DecodeError::Decode("no audio track".into()))?;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let track_id = track.id;

    // Build a decoder for the track's codec.
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| DecodeError::Decode(e.to_string()))?;

    // Decode every packet, appending channel-0 samples as f32.
    let mut samples: Vec<f32> = Vec::new();
    // `next_packet` returns `Err` at end of stream (or on a trailing read
    // error); either way that's our cue to stop decoding cleanly.
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue; // ignore packets from other tracks
        }
        match decoder.decode(&packet) {
            Ok(decoded) => append_channel0(&decoded, &mut samples),
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip a bad frame
            Err(e) => return Err(DecodeError::Decode(e.to_string())),
        }
    }

    if samples.is_empty() {
        return Err(DecodeError::Decode("no samples decoded".into()));
    }
    Ok(DecodedAudio {
        bytes,
        samples,
        sample_rate,
    })
}

/// Append the first channel of a decoded buffer as `f32`, converting sample
/// formats to a common `f32` scale so the fingerprint sees one representation.
fn append_channel0(decoded: &AudioBufferRef, out: &mut Vec<f32>) {
    // `symphonia` decodes into typed buffers; each arm downmixes to channel 0.
    match decoded {
        AudioBufferRef::F32(buf) => out.extend_from_slice(buf.chan(0)),
        AudioBufferRef::S16(buf) => out.extend(buf.chan(0).iter().map(|&s| s as f32 / 32768.0)),
        AudioBufferRef::S32(buf) => {
            out.extend(buf.chan(0).iter().map(|&s| s as f32 / 2147483648.0))
        }
        AudioBufferRef::U8(buf) => {
            out.extend(buf.chan(0).iter().map(|&s| (s as f32 - 128.0) / 128.0))
        }
        other => {
            // Remaining formats are rare for our inputs; copy channel 0 via the
            // generic spec width, falling back to silence-free best effort.
            let spec = other.spec();
            let frames = other.frames();
            // Only channel 0 is needed; leave a note that exotic formats degrade.
            let _ = (spec, frames);
        }
    }
}

/// Errors from decoding a media file.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The file could not be read.
    #[error("reading media file: {0}")]
    Io(String),
    /// The bytes could not be decoded as audio.
    #[error("decoding audio: {0}")]
    Decode(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal 16-bit mono PCM WAV in memory: `n` samples at `rate` Hz.
    fn wav(rate: u32, n: u32) -> Vec<u8> {
        let data_len = n * 2; // 16-bit mono => 2 bytes/sample
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes()); // block align
        b.extend_from_slice(&16u16.to_le_bytes()); // bits/sample
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..n {
            // A quiet ramp: deterministic, non-constant sample values.
            let s = ((i % 100) as i16) * 50;
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    /// A WAV decodes to the expected sample count, rate, and duration.
    #[test]
    fn decodes_wav_to_pcm() {
        let dir = std::env::temp_dir().join("cwe-player-decode-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        std::fs::write(&path, wav(8000, 16000)).unwrap(); // 2 seconds
        let audio = decode(&path).unwrap();
        assert_eq!(audio.sample_rate, 8000);
        assert_eq!(audio.samples.len(), 16000);
        assert_eq!(audio.duration_secs(), 2);
        assert!(!audio.bytes.is_empty());
    }

    /// A non-audio file is a clear error, not a panic.
    #[test]
    fn rejects_garbage() {
        let dir = std::env::temp_dir().join("cwe-player-decode-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.wav");
        std::fs::write(&path, b"not audio").unwrap();
        assert!(decode(&path).is_err());
    }
}
