//! `gen-wav` — a dev-only deterministic WAV generator for the demo and tests.
//!
//! Writes `<out>` as 16-bit mono PCM: `seconds` of a fixed-frequency sine at
//! 44.1 kHz. Deterministic, so a work's `content_id = keccak(bytes)` is stable
//! across runs. Usage: `gen-wav <out.wav> <seconds> [freq_hz]`.

use std::f32::consts::PI;

/// Write a little-endian `u32`/`u16`/`i16` sequence building a PCM WAV.
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (out, seconds) = match (args.get(1), args.get(2)) {
        (Some(o), Some(s)) => (o.clone(), s.parse::<u32>().unwrap_or(0)),
        _ => {
            eprintln!("usage: gen-wav <out.wav> <seconds> [freq_hz]");
            return std::process::ExitCode::FAILURE;
        }
    };
    let freq: f32 = args.get(3).and_then(|f| f.parse().ok()).unwrap_or(440.0);
    let rate: u32 = 44_100;
    let n = rate * seconds;
    let data_len = n * 2; // 16-bit mono

    // Assemble the canonical 44-byte RIFF/WAVE header ahead of the PCM payload.
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        // A pure sine at `freq`, scaled to half amplitude — deterministic bytes.
        let t = i as f32 / rate as f32;
        let s = (0.5 * (2.0 * PI * freq * t).sin() * 32767.0) as i16;
        b.extend_from_slice(&s.to_le_bytes());
    }
    if let Err(e) = std::fs::write(&out, b) {
        eprintln!("error writing {out}: {e}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
