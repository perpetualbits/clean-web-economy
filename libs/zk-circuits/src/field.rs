//! Field-element conversions between the BN254 scalar field and native Rust integer/byte types.

use ark_ff::{BigInteger, PrimeField};

/// The BN254 scalar field — the field every circuit value lives in.
pub type Fr = ark_bn254::Fr;

/// Interpret 32 big-endian bytes as an integer and reduce it mod the field order.
/// Used for opaque 256-bit inputs (work id, salt, epoch key) that may exceed `p`.
pub fn fr_from_bytes32(b: &[u8; 32]) -> Fr {
    Fr::from_be_bytes_mod_order(b)
}

/// Encode a field element as canonical big-endian 32 bytes. Field elements are
/// always `< p < 2^254`, so the value fits and the encoding is lossless.
pub fn fr_to_bytes32(f: &Fr) -> [u8; 32] {
    let mut out = [0u8; 32];
    let be = f.into_bigint().to_bytes_be(); // minimal-length big-endian
    out[32 - be.len()..].copy_from_slice(&be);
    out
}

/// Lift a `u128` into the field (always canonical, never reduced).
pub fn fr_from_u128(v: u128) -> Fr {
    Fr::from(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical field element round-trips through bytes32 losslessly.
    #[test]
    fn small_value_round_trips() {
        let f = fr_from_u128(123_456_789);
        let b = fr_to_bytes32(&f);
        assert_eq!(fr_from_bytes32(&b), f);
    }

    /// Big-endian: the integer 1 encodes with its low byte last.
    #[test]
    fn one_is_big_endian() {
        let b = fr_to_bytes32(&fr_from_u128(1));
        assert_eq!(b[31], 1);
        assert_eq!(b[..31], [0u8; 31]);
    }
}
