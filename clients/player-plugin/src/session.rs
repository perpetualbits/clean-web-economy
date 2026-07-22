//! Persistent session state for the player agent.
//!
//! The agent runs as discrete commands, so state that must outlive one process
//! — accrued time (via the shared [`SessionStore`]) and the set of works
//! recognised only by fingerprint (escrow-bound) — is persisted to a single
//! JSON file between invocations, the desktop analogue of the extension's
//! `chrome.storage`.

use std::collections::BTreeSet;
use std::path::Path;

use cwe_wallet_zk::session::{SessionState, SessionStore};
use cwe_wallet_zk::zk::UsageEntry;
use cwe_wallet_zk::Bytes32;
use serde::{Deserialize, Serialize};

/// The full persisted state: the accrual store's state plus the escrow set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerState {
    /// The shared session-store state (epoch + per-work accrued seconds).
    pub session: SessionState,
    /// Works recognised only by fingerprint this epoch — their credit escrows.
    pub escrow_works: BTreeSet<Bytes32>,
}

/// A loaded session: the accrual store plus the escrow set, ready to mutate.
pub struct Session {
    /// The shared accrual store.
    store: SessionStore,
    /// Fingerprint-recognised works to route to escrow at settle time.
    escrow_works: BTreeSet<Bytes32>,
}

impl Session {
    /// Load the session from `path`, or start a fresh one anchored to `now_secs`
    /// when the file is absent. A present-but-unreadable file is an error.
    pub fn load(path: &Path, now_secs: u64) -> Result<Session, SessionError> {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let state: PlayerState =
                    serde_json::from_str(&raw).map_err(|e| SessionError::Parse(e.to_string()))?;
                Ok(Session {
                    store: SessionStore::from_state(state.session),
                    escrow_works: state.escrow_works,
                })
            }
            // No file yet: a brand-new session for the current epoch.
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Session {
                store: SessionStore::new(now_secs),
                escrow_works: BTreeSet::new(),
            }),
            Err(e) => Err(SessionError::Io(e.to_string())),
        }
    }

    /// Persist the current state to `path` (atomically via the parent dir).
    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let state = PlayerState {
            session: self.store.snapshot().clone(),
            escrow_works: self.escrow_works.clone(),
        };
        let json =
            serde_json::to_string_pretty(&state).map_err(|e| SessionError::Parse(e.to_string()))?;
        std::fs::write(path, json + "\n").map_err(|e| SessionError::Io(e.to_string()))
    }

    /// Accrue `secs` of playback to `work_id`. When `fingerprint` is true the
    /// work was recognised only by fingerprint, so it is remembered as
    /// escrow-bound for settlement.
    pub fn accrue(&mut self, work_id: Bytes32, secs: u64, fingerprint: bool) {
        // A single-play session id keyed by the work is enough for one-shot use.
        let sid = format!("play:{work_id}");
        self.store.start(&sid, work_id);
        self.store.add_time(&sid, secs);
        self.store.stop(&sid);
        if fingerprint {
            self.escrow_works.insert(work_id);
        }
    }

    /// Drain the epoch's accrued time into whole-minute usage entries.
    pub fn flush_usage(&mut self) -> Vec<UsageEntry> {
        self.store.flush()
    }

    /// Take (and clear) the escrow-bound work set for inclusion in the disclosure.
    pub fn take_escrow_works(&mut self) -> Vec<Bytes32> {
        std::mem::take(&mut self.escrow_works).into_iter().collect()
    }

    /// A read-only view for `status`: `(epoch, [(work, secs)], escrow_works)`.
    pub fn snapshot_view(&self) -> (u64, Vec<(Bytes32, u64)>, Vec<Bytes32>) {
        let st = self.store.snapshot();
        let per_work = st.per_work_secs.iter().map(|(w, s)| (*w, *s)).collect();
        (
            st.epoch,
            per_work,
            self.escrow_works.iter().copied().collect(),
        )
    }
}

/// Errors loading or saving the session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The state file could not be read/written.
    #[error("session state IO: {0}")]
    Io(String),
    /// The state file was not valid JSON.
    #[error("session state parse: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use cwe_wallet_zk::Bytes32;

    fn b(x: u8) -> Bytes32 {
        Bytes32([x; 32])
    }

    /// Accrued time and the escrow set survive a save/load round-trip.
    #[test]
    fn persists_across_invocations() {
        let path = std::env::temp_dir().join("cwe-player-sess-1.json");
        let _ = std::fs::remove_file(&path);
        {
            let mut s = Session::load(&path, 0).unwrap();
            s.accrue(b(1), 130, false); // signed, 2m10s
            s.accrue(b(2), 200, true); // fingerprint -> escrow-bound
            s.save(&path).unwrap();
        }
        let s2 = Session::load(&path, 0).unwrap();
        let (epoch, per_work, escrow) = s2.snapshot_view();
        assert_eq!(epoch, 0);
        assert!(per_work.contains(&(b(1), 130)));
        assert!(per_work.contains(&(b(2), 200)));
        assert_eq!(escrow, vec![b(2)]); // only the fingerprint work is escrow-bound
    }

    /// Flushing drains usage to floored minutes and take_escrow_works empties it.
    #[test]
    fn flush_and_take() {
        let path = std::env::temp_dir().join("cwe-player-sess-2.json");
        let _ = std::fs::remove_file(&path);
        let mut s = Session::load(&path, 0).unwrap();
        s.accrue(b(1), 130, false);
        s.accrue(b(2), 200, true);
        let usage = s.flush_usage();
        assert_eq!(usage.iter().find(|u| u.work_id == b(1)).unwrap().minutes, 2);
        let escrow = s.take_escrow_works();
        assert_eq!(escrow, vec![b(2)]);
        // After taking, the set is empty (a second take yields nothing).
        assert!(s.take_escrow_works().is_empty());
    }
}
