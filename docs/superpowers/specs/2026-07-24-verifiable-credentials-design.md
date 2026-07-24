<!-- File: docs/superpowers/specs/2026-07-24-verifiable-credentials-design.md -->

# Verifiable Credentials / Identity (H6) — MVP Design

**Status date:** 2026-07-24
**Cycle:** H6 — hardening track (graduate the trusted-allowlist identity to credentials)
**Depends on:** Phase 1 (`CWERegistry`, `Ownable`), Phase 2.3 (`CWEJury`).
**Governing specs:** `docs/specs/content_manifest_and_creator_registration_specification.md`
(§3 Creator Registration), `docs/specs/creator_threat_model.md`, and the SSI/VC
vision in `spec_clean_web_economy_v0.2.md` / `tech_gov_spec_v0.1.md`.

---

## 1. Objective and guiding principle

Replace the owner-managed "verified creator" and "juror" **allowlists** (a bit only
the contract owner can flip) with a real **credential system**: a rotatable set of
trusted **issuers** grant **revocable, expiring** credentials that anyone can verify,
and the registry and jury accept a creator/juror by checking the credential rather
than an admin list.

**Guiding principle — decentralise the trust, keep the seam swappable.** This is the
first real step toward the spec's "Verifiable Credentials (eID/SSI) that bind identity
to earning addresses, revocable, supporting org hierarchies." The MVP builds the
*mechanism* (issue → verify → expire → revoke, on-chain) with the issuer as a role
that can expand or move to governance later, without touching the registry or jury.
The heavier identity infrastructure (real eID, proof-of-personhood, OIDC, W3C
DID/JSON-LD, holder-carried wallet-VCs) are labelled seams, exactly as H3 wired
bandwidth as a seam for H5.

### In scope

- A new `CWEIdentity` contract: an owner-curated **issuer set**; `attest` /`revoke`;
  `isValid(subject, credType)` gated on existence, non-revocation, non-expiry, **and
  the issuing issuer still being trusted**.
- `CWERegistry` and `CWEJury` reworked to accept a creator/juror via
  `CWEIdentity.isValid(...)` instead of their internal allowlists (both allowlists
  removed).
- Deploy wiring, a new `identity-demo` proving the full lifecycle, updated
  registry/escrow/jury test suites and all existing demos, plus a `CWEIdentity` suite.

### Out of scope (deferred seams)

- **Real eID / eIDAS** government attestations; **proof-of-personhood** ("one human,
  one identity").
- **OIDC** login and an off-chain **Identity Service (IDS)**; the full **W3C
  DID/JSON-LD** verifiable-credential format.
- The **holder-carried** (wallet-presented) VC form — the on-chain record is the MVP;
  the wallet-VC is a later refinement at the same seam.
- **Org hierarchies** (labels/studios) — a future credential attribute; deferred (no
  `orgId` field this cycle).
- **Decentralising the issuer set to governance** (Phase 4) and **Governance IDs (GIDs)**.

### Not touched

- Pure Solidity + bash. No Rust changes: the discovery hub already enforces the
  credential transitively (it checks a manifest's signer is the on-chain registrant,
  who must hold a valid credential to have registered), so nothing off-chain changes.

---

## 2. Decisions (locked in brainstorming)

| # | Decision | Choice |
|---|---|---|
| D1 | Credential model | **On-chain record** in `CWEIdentity` (not holder-carried) — tractable, queryable `isValid`, natural expiry/revocation. |
| D2 | Scope | **Creators and jurors** unified under one credential system; both current allowlists removed. |
| D3 | Issuer model | An owner-curated **rotatable set** of issuers; any trusted issuer may `attest`/`revoke`. |
| D4 | Validity rule | `isValid` requires: exists ∧ ¬revoked ∧ `now < expiresAt` ∧ **issuer still trusted**. |
| D5 | Surface | **Pure Solidity**; no Rust; no holder-carried VC; no `orgId` (deferred). |
| D6 | Trust posture | Trusted issuers now; decentralisation to governance is a future swap at the `ICWEIdentity` seam. |

---

## 3. Architecture

### `CWEIdentity.sol` (new)

`is Ownable`. State:
- `mapping(address => bool) public isIssuer;` — the rotatable trusted-issuer set.
- `mapping(bytes32 => Credential) private _credentials;` keyed by
  `keccak256(abi.encode(subject, credType))`.
  ```solidity
  struct Credential {
      address issuer;     // who attested it (checked still-trusted at validity time)
      uint64 issuedAt;    // attestation timestamp
      uint64 expiresAt;   // validity horizon (strict: valid while now < expiresAt)
      bool revoked;       // issuer-revoked flag
      bool exists;        // distinguishes "never attested" from a zeroed record
  }
  ```

Functions:
- `setIssuer(address issuer, bool trusted) external onlyOwner` — curate the issuer set.
- `attest(address subject, bytes32 credType, uint64 expiresAt) external` — **only a
  trusted issuer**; requires `expiresAt > block.timestamp` (`BadExpiry` otherwise);
  records `issuer = msg.sender`, `issuedAt = now`, `revoked = false`, `exists = true`.
  Re-attesting the same `(subject, credType)` overwrites — a renewal.
- `revoke(address subject, bytes32 credType) external` — **only a trusted issuer**;
  sets `revoked = true` (reverts `NoCredential` if none exists).
- `isValid(address subject, bytes32 credType) external view returns (bool)`:
  ```
  c = _credentials[key];
  return c.exists && !c.revoked && block.timestamp < c.expiresAt && isIssuer[c.issuer];
  ```
  The final clause means removing a rogue issuer instantly invalidates **all** their
  credentials (issuer-set revocation).
- View helpers for tests/demos: `credentialOf(subject, credType) → (issuer, issuedAt,
  expiresAt, revoked, exists)`.

Events: `IssuerSet(issuer, trusted)`, `Attested(subject, credType, issuer, expiresAt)`,
`Revoked(subject, credType, issuer)`. Errors: `NotIssuer()`, `BadExpiry()`,
`NoCredential()`.

### `ICWEIdentity.sol` (new) — the seam the registry + jury depend on

```solidity
interface ICWEIdentity {
    /// @notice True iff `subject` holds a currently-valid credential of `credType`
    ///         (exists, not revoked, not expired, from a still-trusted issuer).
    function isValid(address subject, bytes32 credType) external view returns (bool);
}
```

### Credential type tags — `CredentialTypes.sol` (new library, avoids drift)

```solidity
library CredentialTypes {
    bytes32 internal constant VERIFIED_CREATOR = keccak256("cwe.credential.verified-creator");
    bytes32 internal constant JUROR            = keccak256("cwe.credential.juror");
}
```

### `CWERegistry.sol` (modified)

- Constructor gains `ICWEIdentity identity_` (stored `immutable`).
- Remove `isVerifiedCreator`, `setVerifiedCreator`, the `VerifiedCreatorSet` event.
- `registerWork` replaces `if (!isVerifiedCreator[msg.sender]) revert NotVerifiedCreator();`
  with `if (!identity.isValid(msg.sender, CredentialTypes.VERIFIED_CREATOR)) revert NotVerifiedCreator();`
  (keep the `NotVerifiedCreator` error name — the failure mode is unchanged).

### `CWEJury.sol` (modified)

- Constructor gains `ICWEIdentity identity_` (stored `immutable`).
- Remove `isJuror`, `setJuror`, the `JurorSet` event.
- `vote` replaces `if (!isJuror[msg.sender]) revert NotJuror();` with
  `if (!identity.isValid(msg.sender, CredentialTypes.JUROR)) revert NotJuror();`.

### `Deploy.s.sol` (modified)

Deploy order: `CWEIdentity(owner)` → `CWERegistry(owner, identity)` → `CWETiers` /
`CWEConsumption` / `CWEPayouts` (unchanged) → `EarliestRegistrationArbiter(registry)`
→ `CWEJury(owner, arbiter, identity)` → `CWEEscrow(registry, aggregator, jury)` →
`jury.setEscrow(escrow)`. Persist the `identity` address in
`deployments/localhost.json`. (Adding issuers + attesting is demo/test setup, not deploy.)

---

## 4. Migration (the ripple)

Removing the two allowlists is a clean cutover (a local devnet — no deployed state).
Every place that set up a creator or juror changes from a switch to a credential:

- **Tests** (`CWERegistry.t.sol`, `CWEEscrow.t.sol`, `CWEJury.t.sol`): `setUp` deploys
  `CWEIdentity`, wires it into the registry/jury, adds the test owner (or a dedicated
  issuer) via `setIssuer`, and calls `attest(creator, VERIFIED_CREATOR, farFuture)` /
  `attest(juror, JUROR, farFuture)` instead of `setVerifiedCreator`/`setJuror`.
- **Demos** (`run_demo.sh`, `run_hub_demo.sh`, `run_ownership_demo.sh`,
  `run_player_demo.sh`, `run_arbitration_demo.sh`): read `IDENTITY` from the
  deployments JSON, `setIssuer $DEPLOYER true`, then `attest` each creator/juror with a
  far-future expiry, replacing the `setVerifiedCreator`/`setJuror` calls.

---

## 5. The identity demo (`make identity-demo`)

Self-contained, PID-safe, proving the lifecycle the allowlist could not:
1. deploy; `setIssuer(deployer, true)`; **attest** a `verified-creator` credential to
   `CREATOR` (far-future expiry) → `CREATOR` registers a work → **succeeds**;
2. a **non-credentialed** address tries to register → **reverts** `NotVerifiedCreator`;
3. **revoke** `CREATOR`'s credential → a further register/update by `CREATOR` **reverts**;
4. attest a credential with a **near expiry**, warp past it → `isValid` false → register
   **reverts** (expiry works);
5. **`setIssuer(deployer, false)`** (remove the issuer) → a credential that issuer
   granted now reads `isValid == false` → register **reverts** (issuer-set revocation);
6. attest a **`juror`** credential → that address `vote`s in a jury dispute → **succeeds**;
   revoke it → a further `vote` **reverts** `NotJuror`.

Prints `✅ IDENTITY DEMO PASSED` on success; a clear `FAIL:` + `exit 1` otherwise.

---

## 6. Testing

**`CWEIdentity.t.sol` (new):** only the owner curates issuers; only a trusted issuer
`attest`s/`revoke`s (`NotIssuer` otherwise); `attest` rejects a past/zero `expiresAt`
(`BadExpiry`); `isValid` is true only under all four conditions and flips false on
revoke, on crossing `expiresAt` (exact boundary: valid at `now < expiresAt`, invalid at
`==`), and on the issuer being untrusted; re-attest renews; `revoke` on a missing
credential reverts `NoCredential`.

**Updated suites:** `CWERegistry`/`CWEEscrow`/`CWEJury` tests pass with credential
setup; every prior assertion (registration gating, escrow, jury voting) holds through
the new path.

**End-to-end:** `make -C ops identity-demo` passes; `make demo`/`hub-demo`/
`ownership-demo`/`player-demo`/`arbitration-demo`/`antifraud-demo` and the full
`cargo`/`forge` gate stay green.

---

## 7. Risks

| Risk | Mitigation |
|---|---|
| Registry (money-adjacent) now depends on `CWEIdentity` | Blast radius is the same as today's allowlist — it only gates *who may register*; consent signatures still bind payees. A dedicated `CWEIdentity` suite + the final review scrutinise `isValid`. |
| A bug in `isValid` wrongly admits/blocks | Full boundary tests (expiry `<` vs `==`, revoke, issuer-untrusted, non-existent); the four conditions are ANDed with no short-circuit gap. |
| Wide allowlist→credential ripple across tests/demos | Mechanical, done in one cycle; the demos are the end-to-end gate; clean devnet cutover (no migration). |
| Trusted issuers are still trusted parties | Explicitly a stub (D6); expiry + revocation + issuer-set rotation already improve on the owner-toggle, and decentralisation is a future swap at the same seam. |

---

## 8. Deliverable

`make -C ops identity-demo` prints `✅ IDENTITY DEMO PASSED`; both allowlists are gone,
replaced by a credential system with expiry, revocation, and rotatable issuers; all
existing demos and the full gate stay green. The `ICWEIdentity` seam is ready to
graduate toward real eID/SSI, holder-carried VCs, and governance-curated issuers.
