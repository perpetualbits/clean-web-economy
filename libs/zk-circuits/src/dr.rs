//! In-circuit diminishing-returns lookup mirroring the `cwe-dapr` payout curve.

use crate::MAX_PLAYS_CIRCUIT;

/// The diminishing-returns multiplier table: entry `i` is `d_ppm(i+1)` in ppm,
/// generated directly from `cwe_dapr::d_ppm` so the circuit and the payout sim can
/// never disagree. Used both to build the in-circuit constant lookup and to check
/// the prover's native weight.
pub fn d_ppm_table(k_ppm: u64) -> [u64; MAX_PLAYS_CIRCUIT as usize] {
    let mut t = [0u64; MAX_PLAYS_CIRCUIT as usize];
    for (i, slot) in t.iter_mut().enumerate() {
        *slot = cwe_dapr::d_ppm((i as u64) + 1, k_ppm);
    }
    t
}

/// The row's DR-capped weight `raw = minutes·price_ppm·region_ppm · D(plays) / 1e6`,
/// with `plays` clamped to `MAX_PLAYS_CIRCUIT` (1 if zero). Integer math only,
/// mirroring `cwe_dapr`'s `UsageRow::value` then `mul_div(value, d_ppm, 1e6)`.
pub fn weight_of(minutes: u64, plays: u64, price_ppm: u64, region_ppm: u64, k_ppm: u64) -> u128 {
    let v = plays.clamp(1, MAX_PLAYS_CIRCUIT);
    let d = d_ppm_table(k_ppm)[(v - 1) as usize] as u128;
    let value = (minutes as u128) * (price_ppm as u128) * (region_ppm as u128);
    value * d / 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_PLAYS_CIRCUIT;

    /// The table entries equal the sim's d_ppm for plays 1..=MAX.
    #[test]
    fn table_matches_sim() {
        let k = 1_000_000;
        let t = d_ppm_table(k);
        for j in 1..=MAX_PLAYS_CIRCUIT {
            assert_eq!(t[(j - 1) as usize], cwe_dapr::d_ppm(j, k));
        }
    }

    /// weight_of reproduces the sim's raw = value·D(plays)/1e6 for capped plays.
    #[test]
    fn weight_matches_sim_raw() {
        let k = 1_000_000;
        // value = 60 * 1_000_000 * 1_000_000; raw = value * d_ppm(2) / 1e6.
        let value: u128 = 60u128 * 1_000_000 * 1_000_000;
        let expected = value * (cwe_dapr::d_ppm(2, k) as u128) / 1_000_000;
        assert_eq!(weight_of(60, 2, 1_000_000, 1_000_000, k), expected);
    }

    /// Plays above the circuit cap are clamped to the cap (not the sim's 100k cap).
    #[test]
    fn plays_clamped_to_circuit_cap() {
        let k = 1_000_000;
        assert_eq!(
            weight_of(10, 1_000_000, 1_000_000, 1_000_000, k),
            weight_of(10, MAX_PLAYS_CIRCUIT, 1_000_000, 1_000_000, k)
        );
    }
}
