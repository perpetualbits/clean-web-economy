//! The price-cap policy: refuse to accrue a work priced above the user's cap.
//!
//! Mirrors the browser extension's `policy.js`: an unset threshold allows any
//! price; otherwise the work's per-minute price must not exceed the cap.

/// Whether a work priced at `price_per_min` is allowed under `threshold`.
/// `None` means "no cap set" and allows everything.
pub fn allows(price_per_min: u64, threshold: Option<u64>) -> bool {
    match threshold {
        // A set cap admits prices at or below it; anything dearer is blocked.
        Some(cap) => price_per_min <= cap,
        // No cap configured: every price is acceptable.
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No threshold allows any price; a threshold allows prices up to and
    /// including it, and blocks anything above.
    #[test]
    fn threshold_boundaries() {
        assert!(allows(1_000_000, None)); // unset cap allows all
        assert!(allows(500, Some(500))); // equal to the cap is allowed
        assert!(allows(499, Some(500))); // under the cap is allowed
        assert!(!allows(501, Some(500))); // over the cap is blocked
    }
}
