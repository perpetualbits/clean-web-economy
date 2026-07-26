# H5 cycle 3 — node staking and objective fraud proofs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make operating a storage node cost capital, and make one specific lie — attesting
a chunk beyond a work's real extent — objectively provable and slashable by anyone.

**Architecture:** A new `CWEStake` contract holds node bonds and slashes on a submitted
fraud proof. For that proof to be checkable on-chain, receipt signatures move from RFC 8785
canonical JSON to a `keccak256(abi.encode(...))` digest — the pattern `CWERegistry.consentDigest`
already uses. `CWERegistry` gains `contentLength` so "beyond the work's extent" is arithmetic.
The storage node must first be taught to refuse out-of-range chunks, or slashing would
destroy honest operators. Settlement then gates receipts on credential **and** bond.

**Tech Stack:** Solidity/Foundry (`CWEStake`, `CWERegistry`), Rust (`alloy` ABI encoding and
`ecrecover`-compatible EIP-191 signing, axum node, settlement), bash + Anvil (demo).

**Governing spec:** `docs/superpowers/specs/2026-07-26-h5-cycle3-node-staking-design.md`.
Read it before Task 1. §1.1 (what staking cannot buy), §3.2 (the fail-closed rules) and
§3.3 (the honest-node prerequisite) are binding.

## Global Constraints

- **No AI attribution anywhere** — not in code, comments, docs, commit messages, branch
  names, or anything pushed to GitHub. Hard project rule (`CLAUDE.md`).
- **Rust everywhere** except the Solidity under `chain/`.
- **Every function/method gets a `///` doc comment.** Non-trivial lines get an inline
  comment only where it adds understanding, never noise restating the code.
- **Deterministic integer math only** in value-bearing paths. No floating point.
- **The full gate must be green at the END of the branch:** `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `(cd chain && forge test)`. Do **not** run `forge fmt`.
- **Expected red window — Tasks 1 through 5.** Task 1 changes how receipts are signed,
  which breaks `cwe-storage` and `cwe-settlement` until Tasks 4 and 5. Verify with
  **target-scoped** commands in between and do **not** treat a build failure in a target
  another task owns as a finding. `cargo fmt --all -- --check` works throughout.
- **Never kill a process you did not start.** Capture exact PIDs (`cmd & PID=$!`) and kill
  only those. No `pkill`/`killall`/pattern kills.
- Foundry lives at `$HOME/.foundry/bin`; prepend it to `PATH`. `cargo test --workspace` is
  slow (~4 min); scope to `-p <crate>` while iterating.

## Protocol constants (identical everywhere they appear)

```
CHUNK_SIZE     = 131_072            // libs/receipt AND CWEStake — pinned equal by tests
MIN_STAKE      = 10 ether
SLASH_BPS      = 1_000              // 10% of the current bond, per proven receipt
BOUNTY_BPS     = 5_000              // 50% of the slashed amount, to the submitter
UNBOND_DELAY   = 2 * 30 days        // == 2 * CWEConsumption.EPOCH_LENGTH
```

## File Structure

| Path | Responsibility |
|---|---|
| `libs/receipt/src/lib.rs` | `Receipt::digest()` replaces `canonical_bytes()`; signing/recovery over the ABI digest |
| `chain/contracts/CWERegistry.sol` | `contentLength` field, registrant-set setter, getter |
| `chain/contracts/CWEStake.sol` | **New.** bond / requestUnbond / withdraw / slash / isBonded |
| `chain/test/CWEStake.t.sol` | **New.** Bond lifecycle, slashing, the fail-closed rules |
| `services/storage/src/lib.rs` | 404 on an out-of-range chunk; windowed read replacing the whole-file read |
| `services/storage/src/bin/bandwidth_client.rs` | Signs the digest |
| `services/discovery-hub/src/{manifest,chain}.rs` | Manifest mirrors `content_length` |
| `services/settlement/src/{chain,receipts}.rs` | `isBonded` gate; signatures verified before any chain lookup |
| `ops/demo/run_staking_demo.sh`, `ops/Makefile`, `.github/workflows/ci.yml` | `make staking-demo` |
| `ROADMAP.md`, `docs/roadmap.md`, `project-map.js` | Status sync at merge |

---

### Task 1: Receipts become on-chain-verifiable

**Files:** Modify `libs/receipt/src/lib.rs`; tests inline.

**Interfaces produced:**
- `Receipt::digest(&self) -> [u8; 32]` — `keccak256(abi.encode(work_id, consumer, node, chunk_index, bytes, epoch))`
- `Receipt::recover_signer(&self, sig_hex: &str) -> Result<Address, ReceiptError>` — unchanged signature, now recovering over `digest()`
- `canonical_bytes()` is **removed**; `ReceiptError::Canonical` becomes `ReceiptError::Encoding`

**Design notes:**
- Use `alloy::sol_types::SolValue::abi_encode` on a tuple of
  `(FixedBytes<32>, Address, Address, u64, u64, u64)` so the encoding matches Solidity's
  `abi.encode` exactly. Parse the hex strings into `FixedBytes`/`Address` first — encoding
  the *strings* would produce a different digest that Solidity cannot reproduce.
- Signing stays EIP-191 (`sign_message` over the 32-byte digest), matching how
  `CWERegistry`'s consent signatures already work.
- This is the cycle's biggest risk (spec §4): a Rust/Solidity divergence fails **closed**
  (proofs rejected) but silently. The pinned-literal test below is the guard.

- [ ] **Step 1: Write the failing tests**

```rust
    /// The digest is the ABI encoding Solidity will recompute. This literal is
    /// pinned in `chain/test/CWEStake.t.sol` against the SAME field values — if
    /// the two ever diverge, every fraud proof is silently rejected, so both
    /// sides must fail loudly instead.
    #[test]
    fn digest_matches_the_pinned_cross_language_value() {
        let r = Receipt {
            work_id: format!("0x{}", "aa".repeat(32)),
            consumer: format!("0x{}", "11".repeat(20)),
            node: format!("0x{}", "22".repeat(20)),
            chunk_index: 3,
            bytes: 131_072,
            epoch: 7,
        };
        // Recompute with: cast keccak $(cast abi-encode \
        //   "f(bytes32,address,address,uint64,uint64,uint64)" \
        //   0xaaaa...aa 0x1111...11 0x2222...22 3 131072 7)
        assert_eq!(
            format!("0x{}", hex_of(&r.digest())),
            PINNED_RECEIPT_DIGEST,
            "receipt digest changed — update chain/test/CWEStake.t.sol to match, or \
             fraud proofs will be silently rejected"
        );
    }

    /// A round-trip still verifies over the new preimage.
    #[test]
    fn co_signed_receipt_verifies_over_the_digest() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let r = sample(
            &format!("{:#x}", consumer.address()),
            &format!("{:#x}", node.address()),
        );
        co_sign(r, &node, &consumer).verify().unwrap();
    }

    /// Changing any field changes the digest, so a tampered receipt cannot keep
    /// a valid signature.
    #[test]
    fn every_field_is_bound_into_the_digest() {
        let base = sample("0x1111111111111111111111111111111111111111",
                          "0x2222222222222222222222222222222222222222");
        let d = base.digest();

        let mut a = base.clone(); a.chunk_index += 1;
        let mut b = base.clone(); b.bytes += 1;
        let mut c = base.clone(); c.epoch += 1;
        let mut e = base.clone(); e.work_id = format!("0x{}", "ab".repeat(32));
        for other in [a, b, c, e] {
            assert_ne!(d, other.digest());
        }
    }
```

Add above the test module:

```rust
    /// The digest of the fixture in `digest_matches_the_pinned_cross_language_value`,
    /// duplicated verbatim in `chain/test/CWEStake.t.sol`.
    const PINNED_RECEIPT_DIGEST: &str = "<compute in Step 3 and paste here>";
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p cwe-receipt`
Expected: FAIL — no method `digest`.

- [ ] **Step 3: Implement `digest` and derive the pinned literal**

Replace `canonical_bytes` with:

```rust
    /// The 32-byte digest both parties sign, and the one `CWEStake` recomputes
    /// on-chain when checking a fraud proof.
    ///
    /// `keccak256(abi.encode(work_id, consumer, node, chunk_index, bytes, epoch))`
    /// — the same shape `CWERegistry.consentDigest` uses for payee consent. The
    /// hex strings are parsed into their binary forms first: encoding the
    /// STRINGS would yield a digest Solidity cannot reproduce, and the failure
    /// would be silent.
    pub fn digest(&self) -> [u8; 32] {
        use alloy::primitives::{keccak256, Address, FixedBytes};
        use alloy::sol_types::SolValue;

        // A malformed field cannot produce a meaningful digest; zero is used so
        // the function stays infallible, and any signature over it simply fails
        // to recover to the named party.
        let work: FixedBytes<32> = self.work_id.parse().unwrap_or(FixedBytes::ZERO);
        let consumer: Address = self.consumer.parse().unwrap_or(Address::ZERO);
        let node: Address = self.node.parse().unwrap_or(Address::ZERO);

        let encoded = (work, consumer, node, self.chunk_index, self.bytes, self.epoch)
            .abi_encode();
        keccak256(encoded).0
    }
```

and point `recover_signer` at it:

```rust
        let sig = Signature::try_from(bytes.as_slice()).map_err(|_| ReceiptError::BadSignature)?;
        sig.recover_address_from_msg(self.digest())
            .map_err(|e| ReceiptError::Recover(e.to_string()))
```

Rename `ReceiptError::Canonical(String)` to `ReceiptError::Encoding(String)` (it now only
covers bundle JSON), drop the `serde_jcs` dependency from `libs/receipt/Cargo.toml`, and
remove the now-stale `canonical_bytes_are_stable` test.

Then obtain the literal — run the test once, take the `left` value from the assertion
failure, paste it into `PINNED_RECEIPT_DIGEST`, and **independently confirm it** with:

```bash
export PATH="$HOME/.foundry/bin:$PATH"
cast keccak $(cast abi-encode "f(bytes32,address,address,uint64,uint64,uint64)" \
  0x$(printf 'aa%.0s' {1..32}) \
  0x$(printf '11%.0s' {1..20}) \
  0x$(printf '22%.0s' {1..20}) 3 131072 7)
```

Both must agree. If they do not, the Rust encoding is wrong — fix it rather than pasting
the Rust value, or the on-chain check will never match.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p cwe-receipt`
Expected: PASS, including the pinned digest.

- [ ] **Step 5: Commit**

```bash
git add libs/receipt Cargo.lock
git commit -m "receipt: sign an ABI digest so a receipt can be verified on-chain"
```

---

### Task 2: `CWERegistry.contentLength`

**Files:** Modify `chain/contracts/CWERegistry.sol`; test `chain/test/CWERegistry.t.sol`.

**Interfaces produced:**
- `setContentLength(bytes32 workId, uint256 length) external` — registrant only, `length > 0`
- `contentLengthOf(bytes32 workId) external view returns (uint256)` — `0` when unset
- `error BadContentLength();`, `event ContentLengthSet(bytes32 indexed workId, uint256 length)`

**Design notes:** Purely additive, exactly as `bandwidthRate` was — `registerWork`'s
signature must not change. The existing `error NotRegistrant()` covers both the wrong-caller
and unregistered-work cases (an unregistered work's `registrant` is the zero address).
Reject `length == 0` on set, so "unset" and "set to zero" cannot be confused: Task 3 treats
`0` as *unknown extent* and must refuse to slash on it.

- [ ] **Step 1: Write the failing tests**

Append to `chain/test/CWERegistry.t.sol`, reusing the file's existing `WORK`, `creator` and
`other` fixtures and registering the work first the way `test_update_onlyRegistrant` does:

```solidity
    /// @notice An unset content length reads back as zero, which CWEStake treats
    ///         as "extent unknown" and refuses to slash on.
    function test_contentLength_defaultsToZero() public {
        assertEq(registry.contentLengthOf(WORK), 0);
    }

    /// @notice The registrant can set it and read it back.
    function test_contentLength_registrantCanSet() public {
        vm.prank(creator);
        registry.setContentLength(WORK, 4_194_304);
        assertEq(registry.contentLengthOf(WORK), 4_194_304);
    }

    /// @notice Zero is refused on set, so "never set" and "set to zero" stay
    ///         distinguishable — CWEStake's refusal to slash depends on it.
    function test_contentLength_rejectsZero() public {
        vm.prank(creator);
        vm.expectRevert(CWERegistry.BadContentLength.selector);
        registry.setContentLength(WORK, 0);
    }

    /// @notice Only the registrant may set it.
    function test_contentLength_onlyRegistrant() public {
        vm.prank(other);
        vm.expectRevert(CWERegistry.NotRegistrant.selector);
        registry.setContentLength(WORK, 4_194_304);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd chain && export PATH="$HOME/.foundry/bin:$PATH" && forge test --match-test contentLength`
Expected: FAIL — member not found.

- [ ] **Step 3: Implement**

Add `uint256 contentLength;` to the `Work` struct (appended, so existing fields keep their
slots), then beside `setBandwidthRate`:

```solidity
    /// @notice A work's content length in bytes, or 0 if never set.
    error BadContentLength();

    /// @notice Emitted when a work's content length is set or changed.
    event ContentLengthSet(bytes32 indexed workId, uint256 length);

    /// @notice Declare how many bytes this work's content is.
    /// @dev Registrant-only. `CWEStake` uses it to decide whether a receipt names
    ///      a chunk beyond the work's real extent, so zero is refused here: an
    ///      unset length must stay distinguishable from a declared one, and
    ///      `CWEStake` refuses to slash when it is unset rather than treating
    ///      every chunk as out of range.
    function setContentLength(bytes32 workId, uint256 length) external {
        Work storage w = _works[workId];
        // An unregistered work has a zero registrant, so this also rejects
        // setting a length on a work that does not exist.
        if (msg.sender != w.registrant) revert NotRegistrant();
        if (length == 0) revert BadContentLength();
        w.contentLength = length;
        emit ContentLengthSet(workId, length);
    }

    /// @notice The work's declared content length in bytes, or 0 if never set.
    function contentLengthOf(bytes32 workId) external view returns (uint256) {
        return _works[workId].contentLength;
    }
```

- [ ] **Step 4: Run the full contract suite**

Run: `cd chain && forge test`
Expected: PASS, no regressions. Do **not** run `forge fmt`.

- [ ] **Step 5: Commit**

```bash
git add chain/contracts/CWERegistry.sol chain/test/CWERegistry.t.sol
git commit -m "chain: per-work content length, registrant-set and non-zero"
```

---

### Task 3: `CWEStake`

**Files:** Create `chain/contracts/CWEStake.sol` and `chain/test/CWEStake.t.sol`; modify
`chain/script/Deploy.s.sol` to deploy it and write its address into the deployments JSON.

**Interfaces consumed:** `CWERegistry.contentLengthOf` (Task 2); the receipt digest shape
(Task 1).

**Interfaces produced:** `bond()`, `requestUnbond()`, `withdraw()`,
`slash(bytes32 workId, address consumer, address node, uint64 chunkIndex, uint64 numBytes, uint64 epoch, bytes calldata nodeSig)`,
`isBonded(address) view returns (bool)`, and the five constants.

**Design notes — the two rules that must not bend:**
1. **`slash` must revert when `contentLengthOf(workId) == 0`.** With a zero length,
   `chunkIndex * CHUNK_SIZE >= 0` is true for *every* index, so a naive implementation
   makes every receipt for every unrated work slashable — anyone could drain honest nodes.
   This is the cycle-1 `RATE == 0` fail-open in its more dangerous direction.
2. **Widen before multiplying.** `chunkIndex` is `uint64` and `CHUNK_SIZE` ≈ 2¹⁷; a
   `uint64` multiply overflows and could wrap an out-of-range index into a valid-looking one.

- [ ] **Step 1: Write the failing tests**

Create `chain/test/CWEStake.t.sol`. Use `vm.sign` with a known private key so the test can
produce a node signature over the digest, mirroring how `CWERegistry.t.sol` builds consent
signatures.

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEStake} from "../contracts/CWEStake.sol";
import {CWERegistry} from "../contracts/CWERegistry.sol";
import {CWEConsumption} from "../contracts/CWEConsumption.sol";

/// @title CWEStakeTest
/// @notice Bond lifecycle, the objective out-of-range fraud proof, and the two
///         fail-closed rules that keep honest nodes safe.
contract CWEStakeTest is Test {
    CWEStake internal stake;

    uint256 internal nodeKey = 0xA11CE;
    address internal node;
    address internal reporter = makeAddr("reporter");

    bytes32 internal constant WORK = bytes32(uint256(0xW0RK));

    function setUp() public {
        node = vm.addr(nodeKey);
        vm.deal(node, 100 ether);
        // A registry stub returning a fixed content length is enough here; the
        // real integration is exercised by the demo.
        stake = new CWEStake(address(new RegistryStub()));
    }

    /// @notice The constants are exactly the agreed protocol values. The demo
    ///         and the aggregator are calibrated against them.
    function test_constantsArePinned() public {
        assertEq(stake.MIN_STAKE(), 10 ether);
        assertEq(stake.SLASH_BPS(), 1_000);
        assertEq(stake.BOUNTY_BPS(), 5_000);
        // Derived, not chosen: fraud only becomes visible once an epoch settles.
        assertEq(stake.UNBOND_DELAY(), 2 * CWEConsumption.EPOCH_LENGTH());
        assertEq(stake.CHUNK_SIZE(), 131_072);
    }

    /// @notice The digest must match what `cwe-receipt` computes in Rust. This
    ///         literal is pinned identically in `libs/receipt/src/lib.rs`; a
    ///         divergence silently rejects every fraud proof.
    function test_receiptDigest_matchesTheRustPinnedValue() public {
        bytes32 d = stake.receiptDigest(
            bytes32(0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa),
            address(0x1111111111111111111111111111111111111111),
            address(0x2222222222222222222222222222222222222222),
            3, 131_072, 7
        );
        assertEq(d, PINNED_RECEIPT_DIGEST);
    }

    /// @notice A bond at or above the minimum makes a node bonded.
    function test_bond_makesBonded() public {
        assertFalse(stake.isBonded(node));
        vm.prank(node);
        stake.bond{value: 10 ether}();
        assertTrue(stake.isBonded(node));
    }

    /// @notice Requesting unbond stops the node counting IMMEDIATELY, before any
    ///         withdrawal — it must not keep serving while its capital leaves.
    function test_requestUnbond_unbondsImmediately() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        vm.prank(node);
        stake.requestUnbond();
        assertFalse(stake.isBonded(node));
    }

    /// @notice Withdrawal before the delay is refused; after it, the bond returns.
    function test_withdraw_respectsTheDelay() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        vm.prank(node);
        stake.requestUnbond();

        vm.prank(node);
        vm.expectRevert(CWEStake.StillUnbonding.selector);
        stake.withdraw();

        vm.warp(block.timestamp + stake.UNBOND_DELAY() + 1);
        uint256 before = node.balance;
        vm.prank(node);
        stake.withdraw();
        assertEq(node.balance, before + 10 ether);
    }

    /// @notice An out-of-range receipt slashes the node, pays the submitter the
    ///         bounty, and burns the rest.
    function test_slash_onOutOfRangeChunk() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();

        // The stub work is 200_000 bytes: chunks 0 and 1 are real, 2 is not.
        bytes memory sig = _signReceipt(WORK, address(0xC0FFEE), node, 2, 0, 7);

        uint256 reporterBefore = reporter.balance;
        vm.prank(reporter);
        stake.slash(WORK, address(0xC0FFEE), node, 2, 0, 7, sig);

        // 10% of 10 ether slashed; half of that to the reporter.
        assertEq(reporter.balance, reporterBefore + 0.5 ether);
        assertEq(stake.bondOf(node), 9 ether);
    }

    /// @notice An IN-range chunk is not fraud and must not slash.
    function test_slash_rejectsInRangeChunk() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        bytes memory sig = _signReceipt(WORK, address(0xC0FFEE), node, 1, 131_072, 7);
        vm.expectRevert(CWEStake.ChunkInRange.selector);
        stake.slash(WORK, address(0xC0FFEE), node, 1, 131_072, 7, sig);
    }

    /// @notice THE DANGEROUS FAIL-OPEN. With an unset content length every index
    ///         looks out of range, so a naive check would let anyone drain every
    ///         honest node. Slashing must refuse rather than assume.
    function test_slash_refusesWhenContentLengthUnset() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        bytes32 unrated = bytes32(uint256(0xBEEF));
        bytes memory sig = _signReceipt(unrated, address(0xC0FFEE), node, 0, 0, 7);
        vm.expectRevert(CWEStake.ContentLengthUnknown.selector);
        stake.slash(unrated, address(0xC0FFEE), node, 0, 0, 7, sig);
    }

    /// @notice A signature from someone other than the named node proves nothing.
    function test_slash_rejectsForeignSignature() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        uint256 impostorKey = 0xBAD;
        bytes32 d = stake.receiptDigest(WORK, address(0xC0FFEE), node, 2, 0, 7);
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(impostorKey, keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", d)));
        vm.expectRevert(CWEStake.NotTheNode.selector);
        stake.slash(WORK, address(0xC0FFEE), node, 2, 0, 7, abi.encodePacked(r, s, v));
    }

    /// @notice The same proof cannot be submitted twice.
    function test_slash_rejectsReplay() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        bytes memory sig = _signReceipt(WORK, address(0xC0FFEE), node, 2, 0, 7);
        vm.prank(reporter);
        stake.slash(WORK, address(0xC0FFEE), node, 2, 0, 7, sig);
        vm.prank(reporter);
        vm.expectRevert(CWEStake.AlreadySlashed.selector);
        stake.slash(WORK, address(0xC0FFEE), node, 2, 0, 7, sig);
    }

    /// @notice Slashing still works while a node is unbonding, or slash-and-run
    ///         would be free.
    function test_slash_worksDuringUnbonding() public {
        vm.prank(node);
        stake.bond{value: 10 ether}();
        vm.prank(node);
        stake.requestUnbond();
        bytes memory sig = _signReceipt(WORK, address(0xC0FFEE), node, 2, 0, 7);
        vm.prank(reporter);
        stake.slash(WORK, address(0xC0FFEE), node, 2, 0, 7, sig);
        assertEq(stake.bondOf(node), 9 ether);
    }

    /// @notice Enough proven offences drop the bond below the minimum and the
    ///         node de-gates itself — no separate ejection path is needed.
    function test_repeatedSlashing_eventuallyUnbonds() public {
        vm.prank(node);
        stake.bond{value: 11 ether}();
        for (uint64 i = 2; i < 20 && stake.isBonded(node); i++) {
            bytes memory sig = _signReceipt(WORK, address(0xC0FFEE), node, i, 0, 7);
            vm.prank(reporter);
            stake.slash(WORK, address(0xC0FFEE), node, i, 0, 7, sig);
        }
        assertFalse(stake.isBonded(node));
    }

    /// @dev EIP-191-sign a receipt digest with the node's key.
    function _signReceipt(
        bytes32 workId, address consumer, address node_,
        uint64 chunkIndex, uint64 numBytes, uint64 epoch
    ) internal view returns (bytes memory) {
        bytes32 d = stake.receiptDigest(workId, consumer, node_, chunkIndex, numBytes, epoch);
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(nodeKey, keccak256(abi.encodePacked("\x19Ethereum Signed Message:\n32", d)));
        return abi.encodePacked(r, s, v);
    }
}
```

Three corrections to make while transcribing, because the snippet above is illustrative in
these spots:

`bytes32(uint256(0xW0RK))` is not valid Solidity. Declare instead:

```solidity
    bytes32 internal constant WORK = keccak256("cwe.test.work");
    /// @dev The digest of the fixture pinned identically in
    ///      `libs/receipt/src/lib.rs`'s `PINNED_RECEIPT_DIGEST`. Paste the exact
    ///      literal Task 1 produced; the two must never diverge.
    bytes32 internal constant PINNED_RECEIPT_DIGEST = 0x0000000000000000000000000000000000000000000000000000000000000000;
```

replacing the zero literal with Task 1's value. `CWEConsumption.EPOCH_LENGTH` is a public
constant — read it rather than hard-coding 30 days in two places.

And add the stub at the bottom of the same file:

```solidity
/// @notice Minimal stand-in for `CWERegistry`, returning a fixed content length
///         so the staking tests need no full registry deployment. `WORK` is
///         200_000 bytes — chunks 0 and 1 exist, 2 and beyond do not — and every
///         other work is unset, which is what exercises the fail-closed rule.
contract RegistryStub {
    bytes32 internal constant WORK = keccak256("cwe.test.work");

    /// @notice The work's declared content length, or 0 when never set.
    function contentLengthOf(bytes32 workId) external pure returns (uint256) {
        return workId == WORK ? 200_000 : 0;
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd chain && forge test --match-contract CWEStakeTest`
Expected: FAIL — `CWEStake.sol` does not exist.

- [ ] **Step 3: Implement `CWEStake`**

Create `chain/contracts/CWEStake.sol` with the constants from the Global Constraints block,
a `mapping(address => uint256) private _bonds`, a
`mapping(address => uint256) private _unbondAt` (0 = not unbonding), a
`mapping(bytes32 => bool) private _slashed`, and:

- `bond()` — `payable`, adds to `_bonds[msg.sender]`, clears any unbond request.
- `requestUnbond()` — sets `_unbondAt[msg.sender] = block.timestamp + UNBOND_DELAY`.
- `withdraw()` — reverts `StillUnbonding` before the deadline or `NothingToWithdraw` at
  zero; otherwise zeroes the bond and transfers it. Follow checks-effects-interactions and
  use the same reentrancy guard `CWEEscrow` uses.
- `isBonded(node)` — `_bonds[node] >= MIN_STAKE && _unbondAt[node] == 0`.
- `bondOf(node)` — view for tests and the demo.
- `receiptDigest(...)` — `keccak256(abi.encode(workId, consumer, node, chunkIndex, numBytes, epoch))`,
  `public pure`, so both the test and any off-chain caller can compute it.
- `slash(...)` — in this order:
  1. `bytes32 d = receiptDigest(...)`; revert `AlreadySlashed` if `_slashed[d]`.
  2. Recover the EIP-191 signer of `d` from `nodeSig`; revert `NotTheNode` if it differs
     from the `node` argument.
  3. `uint256 len = registry.contentLengthOf(workId)`; **revert `ContentLengthUnknown` if
     `len == 0`** — see the design note; treating unset as "everything is out of range"
     would let anyone drain honest nodes.
  4. Revert `ChunkInRange` unless `uint256(chunkIndex) * CHUNK_SIZE >= len`.
  5. Mark `_slashed[d]`, compute `amount = bond * SLASH_BPS / 10_000`,
     `bounty = amount * BOUNTY_BPS / 10_000`, reduce the bond by `amount`, pay `bounty` to
     `msg.sender`, and burn the remainder by sending it to `address(0)` — or, if the
     project prefers, leave it permanently locked in the contract with a comment saying so.
     Pick one and document it.

Give every function full NatSpec in the file's existing style, and state on `UNBOND_DELAY`
that it is derived from when fraud becomes visible.

- [ ] **Step 4: Run the tests**

Run: `cd chain && forge test --match-contract CWEStakeTest`
Expected: PASS.

- [ ] **Step 5: Deploy it**

In `chain/script/Deploy.s.sol`, deploy `CWEStake` with the registry address and add
`"stake"` to the deployments JSON beside `"identity"`. Then run the whole suite:
`cd chain && forge test` — PASS, no regressions.

- [ ] **Step 6: Commit**

```bash
git add chain/contracts/CWEStake.sol chain/test/CWEStake.t.sol chain/script/Deploy.s.sol
git commit -m "chain: CWEStake — node bonds and an objective out-of-range fraud proof"
```

---

### Task 4: The node must refuse out-of-range chunks

**Files:** Modify `services/storage/src/lib.rs` and `services/storage/src/bin/bandwidth_client.rs`; tests inline in `lib.rs`.

**Interfaces produced:** `fragment_for_chunk` returns `Err(StorageError::ChunkOutOfRange)`
past the end of content; `GET /content/{work_id}` answers `404` for such an index.

**Design notes — this is a prerequisite for Task 3 being safe, not a nicety.** Today an
out-of-range index yields an empty body, an empty `DeliveryStream` fires its completion
callback on the first poll, the ledger records a 0-byte entry, and the node **signs a
receipt for a chunk it does not have**. The moment such receipts are slashable, a client
can request chunk 9999 of a small work and take an honest node's stake. Fix this before
enabling slashing.

While in this function, replace the whole-file `std::fs::read` with a windowed read (open
the file, `seek` to the chunk offset, read at most `CHUNK_SIZE`) — the carry-over finding
from cycle 2, and the same lines are being rewritten anyway.

- [ ] **Step 1: Write the failing tests**

```rust
    /// A chunk index past the end of the content is REFUSED, not served empty.
    /// Without this an honest node signs receipts for chunks it does not have,
    /// and every one of them is slashable under CWEStake.
    #[test]
    fn out_of_range_chunk_is_refused() {
        let dir = std::env::temp_dir().join("cwe-storage-test-oor");
        std::fs::create_dir_all(&dir).unwrap();
        let w = format!("0x{}", "ac".repeat(32));
        // Exactly one full chunk plus 100 bytes: indices 0 and 1 exist, 2 does not.
        std::fs::write(dir.join(format!("{w}.bin")), vec![7u8; CHUNK_SIZE as usize + 100]).unwrap();

        assert_eq!(fragment_for_chunk(&dir, &w, 0).unwrap().len(), CHUNK_SIZE as usize);
        assert_eq!(fragment_for_chunk(&dir, &w, 1).unwrap().len(), 100);
        assert!(matches!(
            fragment_for_chunk(&dir, &w, 2),
            Err(StorageError::ChunkOutOfRange)
        ));
        assert!(matches!(
            fragment_for_chunk(&dir, &w, 9999),
            Err(StorageError::ChunkOutOfRange)
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// An out-of-range request leaves NO ledger entry, so no receipt can be
    /// issued for it. This is the property that keeps an honest node
    /// unslashable: with no signature in existence, there is nothing for
    /// `CWEStake.slash` to verify.
    #[tokio::test]
    async fn out_of_range_request_yields_no_receipt() {
        use tower::ServiceExt;

        let dir = std::env::temp_dir().join("cwe-storage-test-oor-http");
        std::fs::create_dir_all(&dir).unwrap();
        let work = format!("0x{}", "ad".repeat(32));
        // A 1 KiB work: only chunk 0 exists.
        std::fs::write(dir.join(format!("{work}.bin")), vec![7u8; 1024]).unwrap();

        let signer = PrivateKeySigner::random();
        let consumer = format!("{:#x}", PrivateKeySigner::random().address());
        let state = std::sync::Arc::new(NodeState {
            signer,
            content_dir: dir.clone(),
            epoch: 7,
            ledger: std::sync::Mutex::new(Ledger::default()),
        });

        // Chunk 5 does not exist: the node must refuse to serve it.
        let content = router(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/content/{work}?consumer={consumer}&chunk_index=5"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(content.status(), axum::http::StatusCode::NOT_FOUND);

        // And it must therefore refuse to attest it — no ledger entry exists.
        let receipt = router(state)
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/receipt")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({
                            "work_id": work,
                            "chunk_index": 5,
                            "consumer": consumer,
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(receipt.status(), axum::http::StatusCode::NOT_FOUND);

        std::fs::remove_dir_all(&dir).ok();
    }
```

Match the exact `NodeState` construction the neighbouring
`abandoned_http_delivery_yields_no_receipt` uses — if its field set differs from the above,
that test is the authority.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p cwe-storage --lib`
Expected: FAIL — no variant `ChunkOutOfRange`.

- [ ] **Step 3: Implement**

Add `#[error("chunk index is beyond the work's content")] ChunkOutOfRange` to
`StorageError`, and rewrite `fragment_for_chunk`:

```rust
/// Read the `chunk_index`-th `CHUNK_SIZE` block of `work_id`'s content.
///
/// Refuses an index at or past the end of the content with
/// [`StorageError::ChunkOutOfRange`] rather than returning an empty slice. That
/// refusal is load-bearing: `CWEStake` slashes a node that signs a receipt for a
/// chunk beyond a work's real extent, so a node that cheerfully attested empty
/// out-of-range chunks could be drained by any client asking for chunk 9999.
///
/// Reads only the requested window rather than the whole file, so a large work
/// cannot be used to exhaust the node's memory through concurrent requests.
pub fn fragment_for_chunk(
    dir: &Path,
    work_id: &str,
    chunk_index: u64,
) -> Result<Vec<u8>, StorageError> {
    use std::io::{Read, Seek, SeekFrom};

    if !is_work_id(work_id) {
        return Err(StorageError::BadWorkId);
    }
    let path = dir.join(format!("{}.bin", work_id.to_ascii_lowercase()));
    let mut file = std::fs::File::open(&path).map_err(|e| StorageError::Content(e.to_string()))?;
    let len = file
        .metadata()
        .map_err(|e| StorageError::Content(e.to_string()))?
        .len();

    let start = chunk_index.saturating_mul(CHUNK_SIZE);
    // At or past the end is not a short read — it is a chunk this work does not
    // have, and attesting it would be a lie.
    if start >= len {
        return Err(StorageError::ChunkOutOfRange);
    }

    let take = std::cmp::min(CHUNK_SIZE, len - start);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| StorageError::Content(e.to_string()))?;
    let mut buf = vec![0u8; take as usize];
    file.read_exact(&mut buf)
        .map_err(|e| StorageError::Content(e.to_string()))?;
    Ok(buf)
}
```

The `content` handler already returns `404` for any `fragment_for_chunk` error, so no
handler change is needed — confirm that by reading it.

Then update `bandwidth_client.rs` to sign `resp.receipt.digest()` instead of
`canonical_bytes()`, and `services/storage/src/lib.rs`'s receipt handler likewise.

- [ ] **Step 4: Run**

Run: `cargo test -p cwe-storage` and `cargo clippy -p cwe-storage --all-targets -- -D warnings`
Expected: PASS and clean.

- [ ] **Step 5: Commit**

```bash
git add services/storage
git commit -m "storage: refuse out-of-range chunks and read only the requested window"
```

---

### Task 5: Settlement gates on the bond

**Files:** Modify `services/settlement/src/{config.rs,chain.rs,receipts.rs}`; tests inline.

**Interfaces produced:** `Deployments.stake: String`; the node predicate becomes credential
AND `isBonded`.

**Design notes:** Also reorder so **signatures are verified before any chain lookup**. Cycle
2's review flagged that the credential loop runs first, so a bundle of fabricated node
addresses forces one RPC each; adding the bond check doubles that. Verify first, resolve
only surviving nodes.

- [ ] **Step 1: Write the failing test**

In `receipts.rs`'s test module:

```rust
    /// A node with a valid credential but NO bond has its receipts dropped —
    /// the stake genuinely gates, it is not advisory.
    #[test]
    fn drops_receipts_from_an_unbonded_node() {
        let node = PrivateKeySigner::random();
        let consumer = PrivateKeySigner::random();
        let bundle = ReceiptBundle {
            epoch: 5,
            receipts: vec![signed(&node, &consumer, 1000, 5, 0)],
        };
        // Credentialed, but not bonded.
        let got = accept_receipts(&bundle, 5, &|_n: &str| false);
        assert!(got.is_empty());
    }
```

`accept_receipts` already takes a single `credentialed` predicate — keep that signature and
have `chain.rs` pass a closure that is true only when **both** the credential and the bond
hold. That keeps the accept/reject policy synchronous and chain-free.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p cwe-settlement --lib receipts`
Expected: FAIL only if the predicate is not already honoured — if it passes immediately,
say so in the report; the behaviour is inherited and the test is a regression pin.

- [ ] **Step 3: Wire the bond**

Add to `Deployments`:

```rust
    /// The `CWEStake` address (for checking a node's bond alongside its credential).
    pub stake: String,
```

In `chain.rs`, add a `Stake` `sol!` binding with
`function isBonded(address node) external view returns (bool);`, and rewrite the node-resolution
loop so it verifies signatures first:

```rust
            // Verify signatures BEFORE any chain lookup: the bundle is
            // consumer-supplied, so resolving a credential and a bond for every
            // address it names would let a fabricated bundle force two RPC
            // round-trips per fake node.
            let mut nodes: BTreeSet<String> = BTreeSet::new();
            for signed in &bundle.receipts {
                if SignedReceipt::verify(signed).is_ok() {
                    nodes.insert(normalize_addr(&signed.receipt.node));
                }
            }

            let identity = Identity::new(Address::from_str(&cfg.deployments.identity)?, provider);
            let stake = Stake::new(Address::from_str(&cfg.deployments.stake)?, provider);
            let cred_type = storage_node_credential_type();
            let mut valid: BTreeMap<String, bool> = BTreeMap::new();
            for node in nodes {
                let ok = match Address::from_str(&node) {
                    // Both gates must hold: the credential is identity and
                    // revocability, the bond is economic cost.
                    Ok(addr) => {
                        identity.isValid(addr, cred_type).call().await?
                            && stake.isBonded(addr).call().await?
                    }
                    Err(_) => false,
                };
                valid.insert(node, ok);
            }
```

- [ ] **Step 4: Run**

Run: `cargo test -p cwe-settlement` and `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean. Pre-existing tests may need the new `Deployments` field — a
fixture change. If a pre-existing **expected value** changes, stop and report BLOCKED.

- [ ] **Step 5: Commit**

```bash
git add services/settlement
git commit -m "settlement: require an active bond alongside the credential; verify signatures before chain lookups"
```

---

### Task 6: Manifest mirrors `content_length`

**Files:** Modify `services/discovery-hub/src/{manifest.rs,chain.rs}`; fixtures in `api.rs`, `index.rs`.

**Read how `bandwidth_rate` does all five things and follow it precisely** — a structural
divergence from that precedent is the defect to avoid here.

On `WorkManifest`, beside `bandwidth_rate`:

```rust
    /// The work's content length in bytes; MUST equal the on-chain value.
    /// `CWEStake` uses the on-chain figure to decide whether a receipt names a
    /// chunk beyond the work's real extent, so a manifest that disagrees is
    /// rejected rather than trusted.
    pub content_length: u64,
```

In `chain.rs`: add `function contentLengthOf(bytes32 workId) external view returns (uint256);`
to the `sol!` interface beside `bandwidthRateOf`; add `pub content_length: u64` to
`OnChainWork`; populate it in the fetch path with the same `u64::try_from(...)` overflow
guard `bandwidth_rate` uses; add an `IngestError::ContentLengthMismatch` variant; and add
the check immediately after the bandwidth-rate one:

```rust
    // The on-chain length is what CWEStake's fraud proof measures against, so a
    // manifest claiming a different extent is refused outright.
    if m.content_length != on_chain.content_length {
        return Err(IngestError::ContentLengthMismatch);
    }
```

Add a mismatch test modelled exactly on `bandwidth_rate_mismatch_is_rejected` — same
`FakeRegistry`/`manifest` helpers — whose `OnChainWork` matches the `manifest()` helper in
**every** field except `content_length`, so it cannot pass on the wrong mismatch. Update
every `WorkManifest`/`OnChainWork` literal the compiler flags, using one consistent value.

Verify: `cargo test -p cwe-discovery-hub`, `cargo clippy -p cwe-discovery-hub --all-targets -- -D warnings`.

```bash
git add services/discovery-hub
git commit -m "hub: mirror the on-chain content length in signed manifests"
```

---

### Task 7: `make staking-demo` + CI

**Files:** Create `ops/demo/run_staking_demo.sh`; modify `ops/Makefile` and `.github/workflows/ci.yml`.

Model it on `ops/demo/run_bandwidth_demo.sh` — same Anvil startup, PID discipline, deploy
sequence and assertion style. **Each act gets its own node and its own work**, because the
node ledger persists per `(consumer, work_id, chunk_index)` for the process lifetime and
sharing either would let one act inherit another's evidence.

Six acts:

| # | Act | Assert |
|---|---|---|
| 1 | Bonded + credentialed node serves | Its bytes appear in the settlement sidecar and its work is paid |
| 2 | Credentialed but **unbonded** node serves | Sidecar shows 0 bytes for it; its work earns 0 |
| 3 | Node requests unbond, then serves | `isBonded` false immediately; sidecar shows 0 bytes |
| 4 | Submit an out-of-range receipt as a fraud proof | `bondOf` drops 10%; reporter's balance rises by the bounty |
| 5 | Ask an honest node for an out-of-range chunk | HTTP 404, and `POST /receipt` also 404 — **no signature exists to slash with** |
| 6 | Submit the same proof twice | Second reverts |

Act 5 is the safety property that makes slashing tolerable — assert both the content 404
**and** the receipt 404, since only the second proves no signature was produced.

Add the Makefile target and a `staking-e2e` CI job in the style of `bandwidth-e2e`.
`make -C ops staking-demo` must pass; run it. **Do not weaken an assertion to make it
pass** — report BLOCKED with evidence instead.

```bash
git add ops .github/workflows/ci.yml
git commit -m "ops: staking end-to-end demo and its CI job"
```

---

### Task 8: Documentation and status sync

Flip `ROADMAP.md`, `docs/roadmap.md` and `project-map.js` **together** (a `CLAUDE.md`
requirement), today's date **2026-07-26**:

- H5 cycle 3 → done; `CWEStake` added; the demo count becomes **ten**.
- Record §1.1's boundary as a **standing limitation, not a deferral**: a node that
  genuinely hosts content and over-attests to a colluding consumer is undetectable in this
  architecture. Do not write "node fraud is now punished" without it.
- Record that jury adjudication of bandwidth disputes was **rejected**, not deferred, and
  why (no durable evidence for a juror to inspect).
- Name cycle 4 with what remains: ZK bandwidth proof, peer diversity, P2P swarm, ephemeral
  keys, retrievability audits, and permissionless-by-stake admission.
- Both cycle-2 carry-over findings (whole-file read, credential-before-signature) are now
  closed — remove them from the open lists.
- `project.updated` = `2026-07-26`. Do **not** touch `project-map.html`.

Then run the full gate and report its real results:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd chain && export PATH="$HOME/.foundry/bin:$PATH" && forge test)
node --check project-map.js
```

```bash
git add ROADMAP.md docs/roadmap.md project-map.js
git commit -m "Roadmap + project map: H5 cycle 3 (node staking) complete"
```

---

## Verification checklist (before merging)

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `(cd chain && forge test)` all green
- [ ] All **ten** demos pass, including `staking-demo`
- [ ] The pinned receipt digest is identical in `libs/receipt/src/lib.rs` and
      `chain/test/CWEStake.t.sol`, and both match `cast keccak`
- [ ] No pre-existing test's expected value was edited
- [ ] **Mutation check (required):** the demo must *fail* when each of these is
      individually reverted — the bond gate in settlement, the out-of-range 404 in the
      node, and the replay guard in `slash`. Cycle 2 found two of three mutation checks did
      not bite; this is expected practice now.
- [ ] `slash` refuses when `contentLengthOf` is 0 — verified by test, not by inspection
- [ ] No mention of any AI agent, assistant, or vendor anywhere in the diff
