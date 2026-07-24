# Verifiable Credentials / Identity (H6) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the owner-managed "verified creator" and "juror" allowlists with a real credential system — a rotatable issuer set grants revocable, expiring credentials; the registry and jury accept a creator/juror by verifying the credential.

**Architecture:** A new `CWEIdentity` contract (issuer set, attest/revoke, `isValid` with expiry + issuer-still-trusted) behind an `ICWEIdentity` seam; `CWERegistry` and `CWEJury` reworked to check `isValid` instead of their internal allowlists (both allowlists removed). Pure Solidity, no Rust.

**Design spec:** `docs/superpowers/specs/2026-07-24-verifiable-credentials-design.md`.

## Global Constraints

- **Solidity for contracts** (EVM/Foundry), bash for demos — no Rust this cycle (the discovery hub enforces the credential transitively via `registrantOf`, so nothing off-chain changes).
- **No attribution to any coding agent, assistant, or automated tool** anywhere — code, comments, docs, commit messages, branch/PR text. Hard rule.
- **Every function/contract/error/event has a NatSpec doc block**; non-trivial lines get a useful inline comment only where it adds understanding.
- **This modifies the registry (money-adjacent) and the jury.** The blast radius is the same as today's allowlist — it only gates *who may register / vote*; consent signatures still bind payees. The `isValid` boundary conditions (expiry `<` vs `==`, revoke, issuer-untrusted, non-existent) are the review focus.
- **`isValid` = exists ∧ ¬revoked ∧ `now < expiresAt` ∧ `isIssuer[credential.issuer]`** — the four conditions ANDed, no gap.
- `forge build` / `forge test` stay green; the full workspace gate and every existing demo stay green.
- The repo has never run `forge fmt` — match the existing house style; do not reformat.

---

## File Structure

- Create: `chain/contracts/interfaces/ICWEIdentity.sol` — the `isValid` seam.
- Create: `chain/contracts/CredentialTypes.sol` — shared `VERIFIED_CREATOR` / `JUROR` type tags.
- Create: `chain/contracts/CWEIdentity.sol` — the credential registry.
- Create: `chain/test/CWEIdentity.t.sol` — the credential test suite.
- Modify: `chain/contracts/CWERegistry.sol` — remove the allowlist; add `identity` + `isValid` check.
- Modify: `chain/contracts/CWEJury.sol` — remove the allowlist; add `identity` + `isValid` check.
- Modify: `chain/test/{CWERegistry,CWEEscrow,CWEJury,CWEPayouts}.t.sol` — attest credentials in `setUp`.
- Modify: `chain/script/Deploy.s.sol` — deploy `CWEIdentity`, wire it in, persist its address.
- Modify: `ops/demo/run_demo.sh`, `run_hub_demo.sh`, `run_ownership_demo.sh`, `run_player_demo.sh`, `run_arbitration_demo.sh` — attest instead of `setVerifiedCreator`/`setJuror`.
- Create: `ops/demo/run_identity_demo.sh`; Modify: `ops/Makefile`, `.github/workflows/ci.yml`.

Patterns to mirror: `Ownable(initialOwner)` + `onlyOwner` (`chain/contracts/utils/Ownable.sol`); the deploy's owner-guarded broadcast for `setEscrow`; the demo `consent()`/`send`/`warp` helpers.

---

## Task 1: `CWEIdentity` + `ICWEIdentity` + `CredentialTypes`

**Files:**
- Create: `chain/contracts/interfaces/ICWEIdentity.sol`, `chain/contracts/CredentialTypes.sol`, `chain/contracts/CWEIdentity.sol`
- Test: `chain/test/CWEIdentity.t.sol`

**Interfaces:**
- Produces (consumed by Task 2): `ICWEIdentity { function isValid(address subject, bytes32 credType) external view returns (bool); }`; `CredentialTypes.VERIFIED_CREATOR` / `.JUROR`; `CWEIdentity` surface: `isIssuer`, `setIssuer`, `attest`, `revoke`, `isValid`, `credentialOf`.

- [ ] **Step 1: Write `ICWEIdentity.sol` and `CredentialTypes.sol`**

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title ICWEIdentity
/// @notice The credential seam consulted by contracts that gate an action on a
///         verifiable credential (a verified creator, an allowlisted juror, ...).
///         It replaces the per-contract owner allowlists with one queryable
///         source of truth that carries expiry and revocation.
interface ICWEIdentity {
    /// @notice Whether `subject` holds a currently-valid credential of `credType`.
    /// @dev True iff the credential exists, is not revoked, is not past its
    ///      expiry, and was issued by an address that is still a trusted issuer.
    function isValid(address subject, bytes32 credType) external view returns (bool);
}
```

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

/// @title CredentialTypes
/// @notice Canonical credential-type tags, defined once so issuers, the identity
///         contract, and every gating contract agree on the exact bytes32 values.
library CredentialTypes {
    /// @notice A verified content creator, permitted to register works.
    bytes32 internal constant VERIFIED_CREATOR = keccak256("cwe.credential.verified-creator");
    /// @notice An allowlisted juror, permitted to vote in arbitration disputes.
    bytes32 internal constant JUROR = keccak256("cwe.credential.juror");
}
```

- [ ] **Step 2: Write the failing `CWEIdentity` tests**

Create `chain/test/CWEIdentity.t.sol`:

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {CWEIdentity} from "../contracts/CWEIdentity.sol";

contract CWEIdentityTest is Test {
    CWEIdentity internal id;
    address internal owner = makeAddr("owner");
    address internal issuer = makeAddr("issuer");
    address internal alice = makeAddr("alice");
    bytes32 internal constant T = keccak256("cwe.credential.verified-creator");

    function setUp() public {
        id = new CWEIdentity(owner);
        vm.prank(owner);
        id.setIssuer(issuer, true);
        vm.warp(1000); // a sane non-zero clock
    }

    /// @notice A trusted issuer's attestation makes a subject valid until expiry.
    function test_attest_makesValid() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        assertTrue(id.isValid(alice, T));
    }

    /// @notice Only a trusted issuer may attest.
    function test_attest_onlyIssuer() public {
        vm.expectRevert(CWEIdentity.NotIssuer.selector);
        id.attest(alice, T, uint64(block.timestamp + 100));
    }

    /// @notice Attesting a past (or present) expiry is rejected.
    function test_attest_pastExpiry_reverts() public {
        vm.prank(issuer);
        vm.expectRevert(CWEIdentity.BadExpiry.selector);
        id.attest(alice, T, uint64(block.timestamp)); // not strictly in the future
    }

    /// @notice A revoked credential is invalid.
    function test_revoke_invalidates() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        vm.prank(issuer);
        id.revoke(alice, T);
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Revoking a non-existent credential reverts.
    function test_revoke_missing_reverts() public {
        vm.prank(issuer);
        vm.expectRevert(CWEIdentity.NoCredential.selector);
        id.revoke(alice, T);
    }

    /// @notice Validity ends exactly at expiry: valid while now < expiresAt.
    function test_expiry_boundary() public {
        uint64 exp = uint64(block.timestamp + 100);
        vm.prank(issuer);
        id.attest(alice, T, exp);
        vm.warp(exp - 1);
        assertTrue(id.isValid(alice, T));
        vm.warp(exp); // now == expiresAt → invalid
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Removing the issuer invalidates all their credentials.
    function test_untrustedIssuer_invalidates() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100));
        assertTrue(id.isValid(alice, T));
        vm.prank(owner);
        id.setIssuer(issuer, false);
        assertFalse(id.isValid(alice, T));
    }

    /// @notice A re-attest renews (new expiry, cleared revocation).
    function test_reattest_renews() public {
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 10));
        vm.prank(issuer);
        id.revoke(alice, T);
        assertFalse(id.isValid(alice, T));
        vm.prank(issuer);
        id.attest(alice, T, uint64(block.timestamp + 100)); // renew
        assertTrue(id.isValid(alice, T));
    }

    /// @notice A never-attested subject is invalid.
    function test_unknown_isInvalid() public view {
        assertFalse(id.isValid(alice, T));
    }

    /// @notice Only the owner may curate issuers.
    function test_setIssuer_onlyOwner() public {
        vm.expectRevert(); // Ownable.NotOwner
        id.setIssuer(alice, true);
    }
}
```

Run: `cd chain && forge test --match-contract CWEIdentityTest 2>&1 | tail -5` → FAIL (contract not found).

- [ ] **Step 3: Implement `CWEIdentity.sol`**

```solidity
// SPDX-License-Identifier: AGPL-3.0-only
pragma solidity ^0.8.24;

import {ICWEIdentity} from "./interfaces/ICWEIdentity.sol";
import {Ownable} from "./utils/Ownable.sol";

/// @title CWEIdentity
/// @notice A minimal verifiable-credential registry. A rotatable set of trusted
///         issuers `attest` revocable, expiring credentials to subjects; any
///         contract or caller verifies one with `isValid`. It is the Phase-6
///         replacement for per-contract owner allowlists (`setVerifiedCreator`,
///         `setJuror`): the same gate, now portable, expiring, revocable, and
///         verifiable by anyone.
/// @dev Deliberately a *trusted-issuer* stub — the owner curates issuers. Real
///      eID/SSI (proof-of-personhood, OIDC, W3C DID/JSON-LD, holder-carried VCs)
///      graduate this behind the `ICWEIdentity` seam without touching the
///      gating contracts. Removing an issuer invalidates every credential they
///      granted, so a compromised issuer is contained by one `setIssuer` call.
contract CWEIdentity is ICWEIdentity, Ownable {
    /// @notice The rotatable set of addresses permitted to attest/revoke.
    mapping(address => bool) public isIssuer;

    /// @dev A single credential record.
    struct Credential {
        address issuer;   // who attested it (checked still-trusted in isValid)
        uint64 issuedAt;  // attestation time
        uint64 expiresAt; // valid while block.timestamp < expiresAt
        bool revoked;     // issuer-set revocation flag
        bool exists;      // distinguishes "never attested" from a zeroed record
    }

    /// @dev keccak256(subject, credType) => credential.
    mapping(bytes32 => Credential) private _credentials;

    /// @notice Emitted when the owner adds or removes a trusted issuer.
    event IssuerSet(address indexed issuer, bool trusted);
    /// @notice Emitted when an issuer attests a credential.
    event Attested(address indexed subject, bytes32 indexed credType, address indexed issuer, uint64 expiresAt);
    /// @notice Emitted when an issuer revokes a credential.
    event Revoked(address indexed subject, bytes32 indexed credType, address indexed issuer);

    /// @dev Reverts when a non-issuer calls `attest`/`revoke`.
    error NotIssuer();
    /// @dev Reverts when `attest` is given an expiry that is not in the future.
    error BadExpiry();
    /// @dev Reverts when `revoke` targets a credential that was never attested.
    error NoCredential();

    /// @dev Restricts a function to a currently-trusted issuer.
    modifier onlyIssuer() {
        if (!isIssuer[msg.sender]) revert NotIssuer();
        _;
    }

    /// @param initialOwner The address that curates the issuer set.
    constructor(address initialOwner) Ownable(initialOwner) {}

    /// @notice Add or remove a trusted issuer.
    function setIssuer(address issuer, bool trusted) external onlyOwner {
        isIssuer[issuer] = trusted;
        emit IssuerSet(issuer, trusted);
    }

    /// @notice Attest a credential of `credType` to `subject`, valid until
    ///         `expiresAt`. Re-attesting overwrites (a renewal). Issuer-only.
    function attest(address subject, bytes32 credType, uint64 expiresAt) external onlyIssuer {
        if (expiresAt <= block.timestamp) revert BadExpiry();
        _credentials[_key(subject, credType)] = Credential({
            issuer: msg.sender,
            issuedAt: uint64(block.timestamp),
            expiresAt: expiresAt,
            revoked: false,
            exists: true
        });
        emit Attested(subject, credType, msg.sender, expiresAt);
    }

    /// @notice Revoke a subject's credential of `credType`. Issuer-only.
    function revoke(address subject, bytes32 credType) external onlyIssuer {
        Credential storage c = _credentials[_key(subject, credType)];
        if (!c.exists) revert NoCredential();
        c.revoked = true;
        emit Revoked(subject, credType, msg.sender);
    }

    /// @inheritdoc ICWEIdentity
    function isValid(address subject, bytes32 credType) external view returns (bool) {
        Credential storage c = _credentials[_key(subject, credType)];
        // All four must hold: attested, live, unexpired, and from a still-trusted issuer.
        return c.exists && !c.revoked && block.timestamp < c.expiresAt && isIssuer[c.issuer];
    }

    /// @notice The raw credential record for `(subject, credType)` (for tooling/tests).
    function credentialOf(address subject, bytes32 credType)
        external
        view
        returns (address issuer, uint64 issuedAt, uint64 expiresAt, bool revoked, bool exists)
    {
        Credential storage c = _credentials[_key(subject, credType)];
        return (c.issuer, c.issuedAt, c.expiresAt, c.revoked, c.exists);
    }

    /// @dev The storage key binding a subject to a credential type.
    function _key(address subject, bytes32 credType) private pure returns (bytes32) {
        return keccak256(abi.encode(subject, credType));
    }
}
```

- [ ] **Step 4: Run the identity suite to green**

Run: `cd chain && forge test --match-contract CWEIdentityTest -vvv 2>&1 | tail -20` → all PASS.

- [ ] **Step 5: Commit**

```bash
git add chain/contracts/CWEIdentity.sol chain/contracts/interfaces/ICWEIdentity.sol chain/contracts/CredentialTypes.sol chain/test/CWEIdentity.t.sol
git commit -m "Add CWEIdentity credential registry and ICWEIdentity seam"
```

---

## Task 2: Rewire `CWERegistry` + `CWEJury` to credentials

**Files:**
- Modify: `chain/contracts/CWERegistry.sol`, `chain/contracts/CWEJury.sol`
- Test: `chain/test/{CWERegistry,CWEEscrow,CWEJury,CWEPayouts}.t.sol`

**Interfaces:**
- Consumes: `ICWEIdentity` + `CredentialTypes` (Task 1).
- Produces: `CWERegistry` constructor becomes `(address initialOwner, ICWEIdentity identity_)`; `CWEJury` constructor becomes `(address initialOwner, IArbiter fallbackArbiter_, ICWEIdentity identity_)`.

- [ ] **Step 1: Rework `CWERegistry.sol`**

- Import `ICWEIdentity` and `CredentialTypes`.
- Add `ICWEIdentity public immutable identity;`.
- Change the constructor: `constructor(address initialOwner, ICWEIdentity identity_) Ownable(initialOwner) { identity = identity_; }`.
- Remove `mapping(address => bool) public isVerifiedCreator;`, `event VerifiedCreatorSet(...)`, and `function setVerifiedCreator(...)`.
- In `registerWork`, replace `if (!isVerifiedCreator[msg.sender]) revert NotVerifiedCreator();` with:
  ```solidity
  // A creator must hold a currently-valid verified-creator credential.
  if (!identity.isValid(msg.sender, CredentialTypes.VERIFIED_CREATOR)) revert NotVerifiedCreator();
  ```
  (Keep the `NotVerifiedCreator` error — the failure mode is unchanged.)

- [ ] **Step 2: Rework `CWEJury.sol`**

- Import `ICWEIdentity` and `CredentialTypes`.
- Add `ICWEIdentity public immutable identity;`.
- Change the constructor to `(address initialOwner, IArbiter fallbackArbiter_, ICWEIdentity identity_)`, storing both immutables.
- Remove `mapping(address => bool) public isJuror;`, `event JurorSet(...)`, and `function setJuror(...)`.
- In `vote`, replace `if (!isJuror[msg.sender]) revert NotJuror();` with:
  ```solidity
  // A juror must hold a currently-valid juror credential.
  if (!identity.isValid(msg.sender, CredentialTypes.JUROR)) revert NotJuror();
  ```

- [ ] **Step 3: Update the four affected test suites' `setUp`**

Each suite that constructs a registry/jury and allowlists a creator/juror now deploys `CWEIdentity`, wires it in, adds the test owner as an issuer, and attests. The pattern (apply per file, adapting names):

```solidity
import {CWEIdentity} from "../contracts/CWEIdentity.sol";
import {CredentialTypes} from "../contracts/CredentialTypes.sol";
// ...
CWEIdentity internal identity;
// in setUp, replacing `registry.setVerifiedCreator(creator, true);`:
identity = new CWEIdentity(owner);
vm.prank(owner); identity.setIssuer(owner, true);        // owner issues in tests
registry = new CWERegistry(owner, identity);
vm.prank(owner); identity.attest(creator, CredentialTypes.VERIFIED_CREATOR, type(uint64).max);
```

Per-file specifics:
- **`CWERegistry.t.sol`**: as above; every `setVerifiedCreator(X, true)` → `identity.attest(X, VERIFIED_CREATOR, type(uint64).max)` (as the owner-issuer); a `setVerifiedCreator(X, false)` (if any negative test exists) → `identity.revoke(X, VERIFIED_CREATOR)`. Add a test that a non-credentialed sender still reverts `NotVerifiedCreator`.
- **`CWEPayouts.t.sol`** (`setUp` at ~line 67): deploy `identity`, `new CWERegistry(owner, identity)`, issuer=owner, attest the `creator`.
- **`CWEEscrow.t.sol`**: its `setUp` deploys registry + jury; wire `identity` into both (`new CWERegistry(owner, identity)`, `new CWEJury(owner, arbiter, identity)`), attest the creator, and — since escrow tests never vote — no juror credential is needed unless a test calls `vote`.
- **`CWEJury.t.sol`**: deploy `identity`, `new CWEJury(owner, arbiter, identity)`, `new CWERegistry(owner, identity)`; attest the `creator` (VERIFIED_CREATOR) and each juror (JUROR); a `setJuror(j, true)` → `attest(j, JUROR, type(uint64).max)`; any juror-removal test → `revoke(j, JUROR)`.

> `type(uint64).max` as the expiry keeps test credentials effectively non-expiring, so existing assertions are unaffected; the expiry behaviour is covered by `CWEIdentityTest`.

- [ ] **Step 4: Run the full contract suite**

Run: `cd chain && forge test --skip script 2>&1 | tail -12`
Expected: `CWEIdentityTest` + registry/escrow/jury/payouts + the rest PASS. (`Deploy.s.sol` will not compile until Task 3 — it still calls the old constructors; verify with `--skip script`, exactly as the arbitration cycle did.)

- [ ] **Step 5: Commit**

```bash
git add chain/contracts/CWERegistry.sol chain/contracts/CWEJury.sol chain/test/
git commit -m "Registry and jury gate on verifiable credentials, not owner allowlists"
```

---

## Task 3: Deploy wiring + demos + identity demo + CI

**Files:**
- Modify: `chain/script/Deploy.s.sol`, all five `ops/demo/run_*.sh`, `ops/Makefile`, `.github/workflows/ci.yml`
- Create: `ops/demo/run_identity_demo.sh`

- [ ] **Step 1: Wire `CWEIdentity` into `Deploy.s.sol`**

Add `address identity;` to `Deployed`. Deploy `CWEIdentity(d.owner)` before the registry; construct the registry and jury with it; persist the address. Imports: `CWEIdentity`, `ICWEIdentity`.

```solidity
import {CWEIdentity} from "../contracts/CWEIdentity.sol";
import {ICWEIdentity} from "../contracts/interfaces/ICWEIdentity.sol";
// inside run(), before `d.registry = ...`:
d.identity = address(new CWEIdentity(d.owner));
d.registry = address(new CWERegistry(d.owner, ICWEIdentity(d.identity)));
// ... jury now takes identity too:
d.jury = address(new CWEJury(d.owner, EarliestRegistrationArbiter(d.arbiter), ICWEIdentity(d.identity)));
// in _writeDeployments, before the final key:
vm.serializeAddress(obj, "identity", d.identity);
```

Run `cd chain && forge build` → compiles (no `--skip`).

- [ ] **Step 2: Update the five existing demos**

Each demo currently does `send $DEPLOYER $REG "setVerifiedCreator(address,bool)" <addr> true` (and the arbitration demo `setJuror`). Replace with: read `IDENTITY=$(jq -r .identity "$DEP")`, make the deployer an issuer once, then attest with a far-future expiry. Helper lines per demo:

```bash
IDENTITY=$(jq -r .identity "$DEP")
FAR=18446744073709551615   # type(uint64).max — effectively non-expiring
send $DEPLOYER $IDENTITY "setIssuer(address,bool)" $(cast wallet address $DEPLOYER) true
VC=$(cast keccak "cwe.credential.verified-creator")
JUROR=$(cast keccak "cwe.credential.juror")
# replace `setVerifiedCreator($ADDR true)`:
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $ADDR $VC $FAR
# in run_arbitration_demo.sh, replace each `setJuror($JADDR true)`:
send $DEPLOYER $IDENTITY "attest(address,bytes32,uint64)" $JADDR $JUROR $FAR
```

Run each touched demo (`make -C ops demo`, `hub-demo`, `ownership-demo`, `player-demo`, `arbitration-demo`) → each prints its PASS line. Do not proceed until all green.

- [ ] **Step 3: Write `run_identity_demo.sh`**

Self-contained, PID-safe (model on `run_ownership_demo.sh`). It proves the lifecycle the allowlist could not:
1. deploy; read `IDENTITY`, `REG`; `setIssuer(deployer, true)`.
2. `VC=$(cast keccak "cwe.credential.verified-creator")`. Set the tier fee; **attest** `VC` to `CREATOR` (far expiry). `CREATOR` registers a work (with a consenting payee) → **succeeds** (assert the work is registered via `isRegistered`).
3. a **non-credentialed** address tries `registerWork` → assert it **reverts** (capture the failing `cast send` exit / use `cast call`+expect-revert, or assert the work is NOT registered afterward).
4. **revoke**: `revoke(CREATOR, VC)`; `CREATOR` tries to register a second work → **reverts** / not registered.
5. **expiry**: re-attest `CREATOR` with `expiresAt = now + 100`; `warp 200`; a register attempt → **reverts** (assert `isValid(CREATOR, VC)` is now false via `cast call`).
6. **issuer removal**: re-attest afresh (valid again), assert `isValid` true; `setIssuer(deployer, false)`; assert `isValid(CREATOR, VC)` flips **false** (issuer-set revocation).
7. **juror**: re-enable the issuer; `JUROR=$(cast keccak "cwe.credential.juror")`; attest `JUROR` to a juror address; drive a minimal escrow dispute (reuse the ownership/arbitration setup) where that juror `vote`s → **succeeds**; `revoke` the juror credential → a further `vote` **reverts**.
   - If wiring a full dispute is heavy, at minimum assert the juror-credential `isValid` transitions (attest → true, revoke → false) via `cast call`, and note the on-chain `vote` gating is covered by `CWEJuryTest`.

Print `✅ IDENTITY DEMO PASSED` on success; a clear `FAIL: …` + `exit 1` otherwise. Use `callnum`/`send`/`warp`/`consent` helpers copied from the sibling demos.

- [ ] **Step 4: Makefile target + CI job**

`ops/Makefile`: add `identity-demo` to `.PHONY` and a `bash demo/run_identity_demo.sh` target. `.github/workflows/ci.yml`: add an `identity-e2e` job mirroring `ownership-e2e` (checkout, Rust, rust-cache, Foundry, jq, `make -C ops identity-demo`).

- [ ] **Step 5: Full gate + commit**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && ( cd chain && forge test ) && make -C ops identity-demo && make -C ops demo && make -C ops hub-demo && make -C ops ownership-demo && make -C ops player-demo && make -C ops arbitration-demo && make -C ops antifraud-demo` — all green. (Foundry at `$HOME/.foundry/bin`.)

Scan every new/changed file for stray agent/assistant attributions, then:

```bash
git add chain/script/Deploy.s.sol ops/demo/ ops/Makefile .github/workflows/ci.yml
git commit -m "Add identity demo and CI; deploy the credential registry; migrate demos"
```

---

## Self-Review

**Spec coverage:** the credential registry — issuer set, attest/revoke, `isValid` with expiry + issuer-still-trusted (T1); the registry + jury gating on `isValid` with both allowlists removed (T2); deploy wiring, the migration of all five demos, the lifecycle `identity-demo`, and CI (T3). Deferred seams (eID/personhood/OIDC/DID-JSON-LD, holder-carried VC, org hierarchies/`orgId`, governance-curated issuers) are stated, not built — matching the spec.

**Placeholder scan:** the contracts (`ICWEIdentity`, `CredentialTypes`, `CWEIdentity`) and the identity test suite carry full code with concrete boundary assertions (expiry `<` vs `==`, revoke, issuer-untrusted, renew); the registry/jury edits and the demo migration are precise per-file diffs referencing the exact call sites; the demo is an explicit numbered recipe over the sibling helpers. No "TBD"/"add error handling"/"write tests for the above" remain.

**Type consistency:** `ICWEIdentity.isValid` and `CredentialTypes.VERIFIED_CREATOR`/`JUROR` (T1) are consumed by the registry and jury (T2) and by the deploy + demos (T3); the new constructor signatures (`CWERegistry(owner, identity)`, `CWEJury(owner, arbiter, identity)`) are used identically in the tests (T2), the deploy script, and — transitively — every demo (T3); `attest(address,bytes32,uint64)` / `setIssuer(address,bool)` / `revoke(address,bytes32)` selectors match between the contract, the tests, and the demos' `cast send` calls; `type(uint64).max` / `18446744073709551615` is the shared "non-expiring" expiry across tests and demos.
