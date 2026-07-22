//! Epoch-aware session accounting (dev-spec §1.2–1.3).
//!
//! The client tracks listening time *locally* and only ever emits aggregates. A
//! playback session is opened with [`SessionStore::start`], fed elapsed time with
//! [`SessionStore::add_time`], and closed with [`SessionStore::stop`]; time accrues
//! per work. At epoch end [`SessionStore::flush`] drains the accrued seconds into
//! whole-minute usage entries ready to be committed and submitted.
//!
//! The store is *storage-agnostic*: its entire state is the serialisable
//! [`SessionState`], so the browser extension (WP6) can persist a snapshot to
//! `chrome.storage` and restore it, while tests use it purely in memory. An epoch
//! is a fixed 30-day window, matching `CWEConsumption.EPOCH_LENGTH` on-chain (Phase
//! 1 has no beacon; see `docs/specs/epoch_beacon_specification.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::zk::UsageEntry;
use crate::Bytes32;

/// Seconds in one epoch (30 days), matching the on-chain `EPOCH_LENGTH`.
pub const EPOCH_LENGTH_SECS: u64 = 30 * 24 * 60 * 60;

/// The epoch number a Unix timestamp (in seconds) falls into.
pub fn epoch_of(timestamp_secs: u64) -> u64 {
    timestamp_secs / EPOCH_LENGTH_SECS
}

/// The complete, serialisable state of a [`SessionStore`].
///
/// Everything the store needs to survive a restart lives here, so persisting the
/// store is just persisting this value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    /// The epoch the accrued time belongs to.
    pub epoch: u64,
    /// Accrued seconds per work for the current epoch.
    pub per_work_secs: BTreeMap<Bytes32, u64>,
    /// Number of `start`s recorded per work for the current epoch — one per play.
    #[serde(default)]
    pub per_work_plays: BTreeMap<Bytes32, u64>,
    /// Open sessions: session id → the work it is accruing to.
    pub active: BTreeMap<String, Bytes32>,
}

/// Accrues playback time per work within an epoch.
#[derive(Clone, Debug, Default)]
pub struct SessionStore {
    /// The store's persistable state.
    state: SessionState,
}

impl SessionStore {
    /// Create an empty store anchored to the epoch containing `now_secs`.
    pub fn new(now_secs: u64) -> Self {
        SessionStore {
            state: SessionState {
                epoch: epoch_of(now_secs),
                ..Default::default()
            },
        }
    }

    /// Rebuild a store from a persisted snapshot (e.g. loaded from storage).
    pub fn from_state(state: SessionState) -> Self {
        SessionStore { state }
    }

    /// Borrow the current state, e.g. to persist a snapshot.
    pub fn snapshot(&self) -> &SessionState {
        &self.state
    }

    /// The epoch this store is currently accruing into.
    pub fn epoch(&self) -> u64 {
        self.state.epoch
    }

    /// Open a session accruing to `work_id`.
    ///
    /// Re-using a session id simply repoints it at the (possibly new) work; time
    /// added afterwards accrues to that work. Each call counts as one play of
    /// `work_id`, so the play count reflects how many times playback started,
    /// independent of how long each playback lasted.
    pub fn start(&mut self, session_id: impl Into<String>, work_id: Bytes32) {
        self.state.active.insert(session_id.into(), work_id);
        let plays = self.state.per_work_plays.entry(work_id).or_insert(0);
        *plays = plays.saturating_add(1);
    }

    /// Add `dt_secs` of elapsed time to an open session's work.
    ///
    /// Returns `true` if the session was open (and the time applied), `false` if
    /// the session id is unknown — the extension may deliver a stray progress
    /// event after a stop, which is harmless to ignore.
    pub fn add_time(&mut self, session_id: &str, dt_secs: u64) -> bool {
        match self.state.active.get(session_id) {
            Some(&work_id) => {
                // Accrue onto the work's running total, saturating rather than
                // overflowing on absurd inputs.
                let entry = self.state.per_work_secs.entry(work_id).or_insert(0);
                *entry = entry.saturating_add(dt_secs);
                true
            }
            None => false,
        }
    }

    /// Close a session. Accrued time is already recorded, so this only forgets the
    /// session id. Returns `true` if a session was actually open.
    pub fn stop(&mut self, session_id: &str) -> bool {
        self.state.active.remove(session_id).is_some()
    }

    /// Drain the epoch's accrued time and play counts into whole-minute usage
    /// entries.
    ///
    /// Seconds are floored to minutes; works with less than a full minute of
    /// accrued time are dropped (they round to zero), and their play count is
    /// dropped along with them. Both the per-work seconds and per-work play
    /// accumulators are cleared, but open sessions are kept so accrual continues
    /// into the next epoch. Entries come out ordered by work id (from the
    /// `BTreeMap`), giving deterministic, reproducible output.
    pub fn flush(&mut self) -> Vec<UsageEntry> {
        // Take both accumulators, leaving empty maps behind for the next epoch.
        let drained_secs = std::mem::take(&mut self.state.per_work_secs);
        let mut drained_plays = std::mem::take(&mut self.state.per_work_plays);
        drained_secs
            .into_iter()
            .filter_map(|(work_id, secs)| {
                let minutes = secs / 60; // floor seconds to whole minutes
                if minutes == 0 {
                    None // sub-minute usage contributes nothing this epoch
                } else {
                    // Every work with accrued seconds was `start`ed at least once,
                    // so this always finds a count; default to 0 defensively.
                    let plays = drained_plays.remove(&work_id).unwrap_or(0);
                    Some(UsageEntry {
                        work_id,
                        minutes,
                        plays,
                    })
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b32(fill: u8) -> Bytes32 {
        Bytes32([fill; 32])
    }

    /// `epoch_of` floors the timestamp into 30-day windows.
    #[test]
    fn epoch_boundaries() {
        assert_eq!(epoch_of(0), 0);
        assert_eq!(epoch_of(EPOCH_LENGTH_SECS - 1), 0);
        assert_eq!(epoch_of(EPOCH_LENGTH_SECS), 1);
    }

    /// Time accrues to the session's work and flushes as floored minutes, with a
    /// single `start` counting as one play.
    #[test]
    fn accrue_and_flush() {
        let mut store = SessionStore::new(0);
        store.start("s1", b32(0xA));
        assert!(store.add_time("s1", 130)); // 2 min 10 s
        store.stop("s1");

        let usage = store.flush();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].work_id, b32(0xA));
        assert_eq!(usage[0].minutes, 2); // 130 s floors to 2 minutes
        assert_eq!(usage[0].plays, 1); // one start of the session
    }

    /// Two sessions on the same work accumulate into one entry.
    #[test]
    fn multiple_sessions_same_work_sum() {
        let mut store = SessionStore::new(0);
        store.start("s1", b32(0xA));
        store.start("s2", b32(0xA));
        store.add_time("s1", 60);
        store.add_time("s2", 60);
        let usage = store.flush();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].minutes, 2); // 120 s total
    }

    /// Two `start`s on the same work count as two plays; minutes still floor
    /// correctly on the shared accrued seconds.
    #[test]
    fn two_starts_on_same_work_count_two_plays() {
        let mut store = SessionStore::new(0);
        store.start("s1", b32(0xA));
        store.add_time("s1", 60);
        store.stop("s1");
        // A second playback of the same work, e.g. a replay.
        store.start("s1", b32(0xA));
        store.add_time("s1", 90);
        store.stop("s1");

        let usage = store.flush();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].work_id, b32(0xA));
        assert_eq!(usage[0].minutes, 2); // 150 s total floors to 2 minutes
        assert_eq!(usage[0].plays, 2); // two starts of the same work
    }

    /// Sub-minute usage rounds to zero and is dropped from the flush.
    #[test]
    fn sub_minute_usage_dropped() {
        let mut store = SessionStore::new(0);
        store.start("s1", b32(0xA));
        store.add_time("s1", 59);
        assert!(store.flush().is_empty());
    }

    /// Adding time to an unknown session is ignored and reported as such.
    #[test]
    fn add_time_unknown_session_ignored() {
        let mut store = SessionStore::new(0);
        assert!(!store.add_time("nope", 60));
        assert!(store.flush().is_empty());
    }

    /// Flushing clears the accumulator but keeps open sessions accruing.
    #[test]
    fn flush_clears_but_keeps_sessions() {
        let mut store = SessionStore::new(0);
        store.start("s1", b32(0xA));
        store.add_time("s1", 120);
        assert_eq!(store.flush().len(), 1);
        // Session still open: new time accrues and flushes again next epoch.
        store.add_time("s1", 120);
        assert_eq!(store.flush().len(), 1);
    }

    /// The store survives a state snapshot → restore round-trip (persistence).
    #[test]
    fn state_round_trips_through_json() {
        let mut store = SessionStore::new(EPOCH_LENGTH_SECS); // epoch 1
        store.start("s1", b32(0xA));
        store.add_time("s1", 90);

        let json = serde_json::to_string(store.snapshot()).unwrap();
        let restored: SessionState = serde_json::from_str(&json).unwrap();
        let mut store2 = SessionStore::from_state(restored);

        assert_eq!(store2.epoch(), 1);
        let usage = store2.flush();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].minutes, 1);
    }
}
