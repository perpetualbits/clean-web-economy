/*
 * Project map data for the Clean Web Economy.
 *
 * This file is the DATA; project-map.html is the (project-agnostic) RENDERER.
 * To map a different project, keep project-map.html and rewrite this file:
 * define `window.PROJECT_MAP` with the same shape below.
 *
 * Loaded via <script src> (not fetch) so it works when the page is opened
 * directly from disk (file://…/project-map.html), where fetch() of a local
 * file is blocked by the browser.
 *
 * Node status vocabulary:
 *   done    — built, reviewed, merged to main, gated by a passing demo
 *   active  — the cycle currently in flight
 *   planned — on the roadmap, not yet started
 *   seam    — a deliberate stub behind a drop-in interface (swapped in later)
 */
window.PROJECT_MAP = {
  project: {
    name: "Clean Web Economy",
    tagline:
      "A decentralized, privacy-preserving way to pay creators for real attention — without tracking anyone.",
    repo: "perpetualbits/clean-web-economy",
    updated: "2026-07-26",
  },

  // Status vocabulary → label + one-line meaning (colors live in the renderer).
  statuses: {
    done: { label: "Shipped", hint: "Built, reviewed, merged, demo-gated" },
    active: { label: "In progress", hint: "The cycle currently in flight" },
    planned: { label: "Planned", hint: "On the roadmap, not started yet" },
    seam: { label: "Stub · seam", hint: "Deliberate placeholder behind a drop-in interface" },
  },

  // Architectural bands, drawn top → bottom. Data flows: clients → services →
  // contracts, with the core libraries shared by clients and services.
  layers: [
    { id: "clients", label: "Edge clients", hint: "What a listener runs" },
    { id: "services", label: "Off-chain services", hint: "Coordinate, never surveil" },
    { id: "core", label: "Core libraries", hint: "Shared Rust, one implementation" },
    { id: "chain", label: "On-chain contracts", hint: "The rules, once deployed" },
    { id: "ops", label: "Devnet & CI", hint: "One-command proofs" },
  ],

  nodes: [
    // ---- Edge clients ---------------------------------------------------
    {
      id: "browser-ext",
      label: "Browser extension",
      layer: "clients",
      status: "done",
      tags: ["Phase 1", "H1"],
      desc: "A Rust→WASM core behind an MV3 shell: recognizes what's playing, meters it against a price cap, accrues time locally, and submits a usage commitment at settle time.",
      files: ["clients/browser-ext/"],
      parts: [
        { label: "Two-tier recognition", status: "done", desc: "Signed content id first, perceptual fingerprint fallback." },
        { label: "Local accounting + price cap", status: "done", desc: "Per-work time accrual; blocks works over the user's cap." },
        { label: "Playwright e2e", status: "seam", desc: "Phase-1 URL-flow spec is stale vs. the two-tier audio flow." },
      ],
      specs: [{ label: "client architecture", href: "docs/specs/client_architecture_specification.md" }],
      deps: ["wallet-zk", "fingerprint", "discovery-hub"],
    },
    {
      id: "player-plugin",
      label: "Player agent",
      layer: "clients",
      status: "done",
      tags: ["Phase 2·2"],
      desc: "A native Rust desktop agent (cwe-player): decode an audio file, recognize it two-tier, price-cap, accrue, and settle on-chain. The desktop analogue of the extension.",
      files: ["clients/player-plugin/"],
      parts: [
        { label: "symphonia decode", status: "done", desc: "Pure-Rust audio decode → mono PCM + content hash." },
        { label: "play / status / settle / fingerprint", status: "done", desc: "One-shot CLI over the shared session store." },
        { label: "VLC/FFmpeg C module", status: "seam", desc: "The real host integration — a thin FFI shim over this agent." },
        { label: "Video fingerprinting", status: "planned", desc: "Audio-only today." },
      ],
      specs: [{ label: "player plugin design", href: "docs/superpowers/specs/2026-07-21-player-plugin-mvp-design.md" }],
      deps: ["wallet-zk", "fingerprint", "discovery-hub", "settlement"],
    },

    // ---- Off-chain services --------------------------------------------
    {
      id: "discovery-hub",
      label: "Discovery Hub",
      layer: "services",
      status: "done",
      tags: ["Phase 2·1", "H1"],
      desc: "Turns 'I heard this' into 'who to pay' with no per-user state: signed, chain-anchored manifest ingest, content-id (Tier 1) and nearest-fingerprint (Tier 2) resolution, search & trending, OpenAPI.",
      files: ["services/discovery-hub/"],
      parts: [
        { label: "Signed manifest ingest", status: "done", desc: "Only the on-chain registrant can publish; price/region re-verified, and (H5 cycle 2) the manifest's bandwidth_rate must equal CWERegistry.bandwidthRateOf, rejected outright on mismatch." },
        { label: "Tier-1 / Tier-2 resolve", status: "done", desc: "Exact content id, else perceptual nearest-match." },
        { label: "Reputation → ranking", status: "planned", desc: "Consume H3's diversity signal in search/trending (fast-follow)." },
      ],
      specs: [{ label: "discovery hub design", href: "docs/superpowers/specs/2026-07-21-discovery-hub-mvp-design.md" }],
      deps: ["registry"],
    },
    {
      id: "settlement",
      label: "Settlement job",
      layer: "services",
      status: "done",
      tags: ["Phase 1", "H1", "H3", "H2", "H5"],
      desc: "Closes an epoch and routes signed usage to direct payout vs. fingerprint usage to escrow. Dual-mode: legacy disclosure mode opens commitments from a disclosure file and runs DAPR; event mode (H2) pays from the Groth16-proven per-work weights carried on-chain by the proof, with no disclosure file at all. Event mode also verifies a co-signed bandwidth-receipts bundle (H5) — deduped by chunk, floored by a per-user per-epoch evidence minimum, rated from the chain — to drive real per-(user, work) DAPR credibility.",
      files: ["services/settlement/"],
      parts: [
        { label: "DAPR + Merkle root", status: "done", desc: "Per-work credits → committable root + withdrawal proofs." },
        { label: "Direct vs. escrow routing", status: "done", desc: "Signed → CWEPayouts; fingerprint → CWEEscrow." },
        { label: "Dual-mode (disclosure / event)", status: "done", desc: "Legacy disclosure-file path retained for the four pre-ZK demos + player; event mode pays from on-chain proven usage weights (make zk-demo)." },
        { label: "Bandwidth-receipt verification", status: "done", desc: "Verifies both signatures, the storage-node credential, and anti-replay on a receipts bundle; H5 cycle 2 added chunk-index dedup and the per-user per-epoch evidence floor on top; sums verified bytes per (user, work) into a real credibility_ppm for cwe-dapr. Absent → neutral, legacy demos unchanged." },
        { label: "RATE(W) sourcing", status: "done", desc: "H5 cycle 2: read from the on-chain CWERegistry.bandwidthRate, protocol-clamped to [60_000, 2_000_000_000] bytes/1e12-weight, mirrored in the signed manifest; the aggregator's RATES config file is retired. Still fails closed on a missing/zero rate." },
        { label: "Migrate legacy demos off disclosure", status: "planned", desc: "H2 cycle 2: move the four legacy demos + player onto real proofs, retiring the disclosure path." },
        { label: "Decentralised settlement", status: "planned", desc: "H4: rollup / multi-aggregator instead of one trusted aggregator." },
        { label: "Credential lookup precedes signature check", status: "seam", desc: "Still resolves a receipt's node credential before verifying its signature; an RPC error aborts settlement outright — a live DoS surface once consumers submit bundles directly, unresolved this cycle." },
      ],
      specs: [
        { label: "DAPR protocol", href: "docs/specs/DAPR_usage_aggregation_protocol.md" },
        { label: "H5 bandwidth receipts design (cycle 1)", href: "docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md" },
        { label: "H5 credibility integrity design (cycle 2)", href: "docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md" },
      ],
      deps: ["dapr", "wallet-zk", "registry", "payouts", "escrow", "verifier", "epoch-beacon", "storage-node", "receipt"],
    },
    {
      id: "storage-node",
      label: "Storage node (cwe-storage)",
      layer: "services",
      status: "done",
      tags: ["H5"],
      desc: "A minimal HTTP node that serves real content fragments by chunk index, keeps a ledger keyed by (consumer, work, chunk_index) that credits a chunk only once fully delivered, and co-signs a bandwidth receipt for each served chunk — never attesting bytes its own ledger didn't record.",
      files: ["services/storage/"],
      parts: [
        { label: "HTTP fragment surface", status: "done", desc: "/health, /content/{work_id}?chunk_index=… (chunk-indexed read + ledger record), /receipt (co-sign a fully-delivered chunk)." },
        { label: "Delivery-gated ledger", status: "done", desc: "H5 cycle 2: credits a chunk only once it has been streamed to completion (DeliveryStream); a transfer abandoned part-way credits nothing. A receipt request for an unrecorded chunk is refused, never fabricated." },
        { label: "Storage-node credential", status: "done", desc: "Signs with a CWEIdentity storage-node-credentialed key; an uncredentialed node's receipts are rejected by settlement. A colluding credentialed node can still sign for chunk indices beyond a work's true extent — that boundary is unchanged by cycle 2." },
        { label: "P2P storage swarm", status: "planned", desc: "IPFS/torrent distribution, redundancy, availability/proof-of-storage, node discovery — still a single plain-HTTP node." },
        { label: "Node compliance & staking/slashing", status: "planned", desc: "AFBRP §6.3 / storage-node policy §8-§12 — deferred; this is what would bind a colluding credentialed node, which cycle 2's per-work cap does not." },
        { label: "Whole-file read in fragment_for_chunk", status: "seam", desc: "Still reads a work's entire content file into memory per request regardless of the requested chunk — a memory-exhaustion vector under concurrent load, unresolved this cycle." },
      ],
      specs: [
        { label: "H5 bandwidth receipts design (cycle 1)", href: "docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md" },
        { label: "H5 credibility integrity design (cycle 2)", href: "docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md" },
      ],
      deps: ["receipt", "identity"],
    },
    {
      id: "creator-portal",
      label: "Creator portal / DMF",
      layer: "services",
      status: "planned",
      tags: ["Phase 3"],
      desc: "The Distributed Microservice Fabric: creator shops, gigs/commissions, split-pay, a signed service registry, and SSI/OIDC auth. Gated by H6 identity.",
      files: ["services/creator-portal/"],
      parts: [],
      specs: [{ label: "dev-friendly spec", href: "docs/dev_friendly_spec_v0.2.md" }],
      deps: ["identity"],
    },

    // ---- Core libraries -------------------------------------------------
    {
      id: "fingerprint",
      label: "Perceptual fingerprint",
      layer: "core",
      status: "done",
      tags: ["H1"],
      desc: "A Haitsma-Kalker acoustic fingerprint — gain-invariant, compared by Hamming distance — that recognizes a work from its audio. The cautious Tier-2 fallback when content isn't signed.",
      files: ["libs/fingerprint/"],
      parts: [
        { label: "Production robustness", status: "planned", desc: "Re-encode resilience, landmark/chromaprint — a later hardening pass." },
      ],
      specs: [{ label: "fingerprinting spec", href: "docs/specs/fingerprinting_specification.md" }],
      deps: [],
    },
    {
      id: "wallet-zk",
      label: "Client core (wallet-zk)",
      layer: "core",
      status: "done",
      tags: ["Phase 1", "H3", "H2"],
      desc: "Keccak usage commitments (binding work + minutes + plays) and Poseidon commitments shared with the ZK circuit, the epoch session store shared by both clients, and the ZK seam. The trust primitive under all usage reporting.",
      files: ["libs/wallet-zk/"],
      parts: [
        { label: "Usage commitments", status: "done", desc: "Hiding commitment over (work, minutes, plays, salt); keccak (legacy) and Poseidon (ZK path) both supported." },
        { label: "Session store", status: "done", desc: "Per-work time + play counting; snapshot/restore." },
        { label: "Real ZK proofs", status: "done", desc: "H2 cycle 1: a Groth16/BN254 circuit (cwe-zk-circuits) behind IProofVerifier, verified on-chain — replaces the disclosure-file stand-in on the event-mode path." },
      ],
      specs: [{ label: "ZK usage proof reqs", href: "docs/specs/zk_usage_proof_requirements.md" }],
      deps: [],
    },
    {
      id: "zk-circuits",
      label: "ZK usage circuits",
      layer: "core",
      status: "done",
      tags: ["H2"],
      desc: "A Groth16/BN254 circuit proving a user's per-epoch usage is well-formed, range-bounded, diminishing-returns-capped, epoch-bound, and per-work unique, exposing only a Poseidon digest on-chain.",
      files: ["libs/zk-circuits/"],
      parts: [
        { label: "Circuit + Poseidon commit/pseudonym/digest", status: "done", desc: "R1CS constraints for range, DR-cap, epoch-binding, and per-work uniqueness." },
        { label: "Devnet trusted setup + prover", status: "done", desc: "Fixed-seed Groth16 keygen (INSECURE, devnet-only — toxic waste is a known seed) plus witness proving." },
        { label: "EVM proof-point export", status: "done", desc: "arkworks -> Solidity calldata encoding, round-tripped against Groth16Verifier." },
        { label: "Manifest-signature / tier-eligibility circuits", status: "planned", desc: "H2 cycle 2: prove signed-manifest authenticity and tier entitlement in-circuit." },
        { label: "Work-identity blinding + cross-epoch unlinkability", status: "planned", desc: "H2 cycle 2: hide which works a user consumed and unlink proofs across epochs." },
        { label: "Real randomness beacon (VRF/drand)", status: "planned", desc: "H2 cycle 2: today's epoch key (CWEEpochBeacon) is owner-published, not randomness-backed." },
      ],
      specs: [{ label: "ZK usage proofs design", href: "docs/superpowers/specs/2026-07-24-zk-usage-proofs-design.md" }],
      deps: ["wallet-zk"],
    },
    {
      id: "dapr",
      label: "Payout math (DAPR)",
      layer: "core",
      status: "done",
      tags: ["Phase 1", "H3", "H5"],
      desc: "The fair-split brain (cwe-dapr): user-centric split with diminishing returns on repeats, a bandwidth-credibility discount now driven by real per-(user, work) receipts, and a reputation signal — deterministic integer math, fee-conserving.",
      files: ["sims/"],
      parts: [
        { label: "Diminishing returns", status: "done", desc: "The j-th replay counts for 1/j; caps replay inflation." },
        { label: "Bandwidth discount", status: "done", desc: "H5: per-row credibility_ppm from verified receipts (neutral default reproduces prior payouts bit-for-bit); a claim backed by zero verified bytes is a strict loss (fee burned)." },
        { label: "Reputation signal", status: "done", desc: "Distinct-listener breadth for discovery ranking." },
        { label: "Chunk dedup + delivery-gated crediting", status: "done", desc: "H5 cycle 2: receipts bind chunk_index and dedup on (consumer, work_id, chunk_index), so re-fetching one chunk credits it once; the storage node credits a chunk only once fully delivered. Closes cycle 1's fetch-and-discard gap — measured in review as 25 MiB of verified bytes from an 8 MiB file, now impossible." },
        { label: "Per-epoch evidence floor", status: "done", desc: "H5 cycle 2: an absolute per-user, per-epoch floor (MIN_EPOCH_BYTES, one 131_072-byte chunk) multiplies the per-row ratio. Closes cycle 1's dust-weight gap: a claim that previously cleared at full credibility off one byte now burns 99.9993% of its fee." },
        { label: "Transport-accepted, not application-consumed", status: "seam", desc: "Crediting proves bytes left the server and were accepted by the client's transport, not that the application did anything with them. Permanently out of scope, not deferred: unachievable over HTTP and not a meaningful distinction for a bandwidth measure — a client that receives and discards has still paid the bandwidth." },
        { label: "Sub-chunk works partially discounted", status: "seam", desc: "Because the floor is absolute, a user whose entire epoch is a single work smaller than one chunk (~8s of audio) is partially discounted despite consuming it fully — a deliberate trade; a relative floor would remove the penalty but also hands a dust claimant a factor of 1.0." },
        { label: "Cross-work floor is a cost, not a barrier", status: "seam", desc: "The floor sums a user's evidence across ALL works, so 128 KiB of any content satisfies it. A dust claimant still needs one verified byte of the claimed work plus 128 KiB of anything else per epoch — a real recurring per-account cost that makes farming uneconomic at scale, not an absolute barrier to one determined claimant." },
        { label: "Per-work credit cap binds a client, not a colluding node", status: "seam", desc: "Against a client acting alone, credit for (user, work) is bounded by the work's real size. It does not bind a colluding credentialed node, which can sign for chunk indices beyond a work's true extent — that is what the CWEIdentity storage-node credential gates today, and what staking/slashing (still deferred) is for." },
      ],
      specs: [
        { label: "full DAPR + anti-fraud", href: "docs/superpowers/specs/2026-07-22-full-dapr-antifraud-design.md" },
        { label: "H5 bandwidth receipts design (cycle 1)", href: "docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md" },
        { label: "H5 credibility integrity design (cycle 2)", href: "docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md" },
      ],
      deps: ["receipt"],
    },
    {
      id: "receipt",
      label: "Bandwidth receipts (cwe-receipt)",
      layer: "core",
      status: "done",
      tags: ["H5"],
      desc: "The co-signed Receipt tuple that proves content bytes actually moved: binds a content position (chunk_index), canonical RFC 8785 bytes, EIP-191 signing/recovery, and the (consumer, work_id, chunk_index) anti-replay/credit dedup key. Portable — no I/O, no chain calls — shared by the storage node, the consumer, and settlement.",
      files: ["libs/receipt/"],
      parts: [
        { label: "Receipt type + canonical bytes", status: "done", desc: "(work_id, consumer, node, chunk_index, bytes, epoch); RFC 8785 JCS so both signers encode identically. session_nonce/chunk_nonce removed in cycle 2, not retained." },
        { label: "EIP-191 sign / recover", status: "done", desc: "Shared by the node's serving signature and the consumer's counter-signature." },
        { label: "Content-position dedup key", status: "done", desc: "H5 cycle 2: (consumer, work_id, chunk_index) — each block of a work counts once per consumer per epoch, however many times it's re-requested. CHUNK_SIZE = 131_072 bytes, a protocol constant." },
      ],
      specs: [
        { label: "H5 bandwidth receipts design (cycle 1)", href: "docs/superpowers/specs/2026-07-25-h5-bandwidth-receipts-design.md" },
        { label: "H5 credibility integrity design (cycle 2)", href: "docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md" },
      ],
      deps: [],
    },

    // ---- On-chain contracts --------------------------------------------
    {
      id: "identity",
      label: "CWEIdentity (credentials)",
      layer: "chain",
      status: "done",
      tags: ["H6", "H5"],
      desc: "Replaces the owner-managed 'verified creator' and juror allowlists with revocable, expiring verifiable credentials: a rotatable issuer set attests; the registry, jury, and (since H5) the settlement aggregator verify. Removing an issuer invalidates all their credentials.",
      files: ["chain/contracts/CWEIdentity.sol", "chain/contracts/interfaces/ICWEIdentity.sol", "chain/contracts/CredentialTypes.sol"],
      parts: [
        { label: "Issue / verify / expire / revoke", status: "done", desc: "isValid = exists ∧ ¬revoked ∧ ¬expired ∧ issuer still trusted." },
        { label: "Storage-node credential", status: "done", desc: "H5: a storage-node credential type; settlement counts only receipts co-signed by a credentialed node." },
        { label: "eID / proof-of-personhood / OIDC", status: "seam", desc: "Real identity infra deferred; issuer as a rotatable role for now." },
      ],
      specs: [{ label: "verifiable credentials (H6)", href: "docs/superpowers/specs/2026-07-24-verifiable-credentials-design.md" }],
      deps: [],
    },
    {
      id: "registry",
      label: "CWERegistry",
      layer: "chain",
      status: "done",
      tags: ["Phase 1", "H1", "H5"],
      desc: "The work registry: content_id, multi-party consent (each payee signs their share), a registration timestamp used as the dispute priority key, and (H5 cycle 2) each work's bandwidth rate.",
      files: ["chain/contracts/CWERegistry.sol"],
      parts: [
        { label: "content_id + timestamp", status: "done", desc: "Authoritative signed ownership + earliest-registration key." },
        { label: "Multi-party consent", status: "done", desc: "Every payee EIP-191 signs their exact split." },
        { label: "Credential-gated", status: "done", desc: "H6: registers only with a valid verified-creator credential (allowlist removed)." },
        { label: "bandwidthRate", status: "done", desc: "H5 cycle 2: an additive, registrant-only setter/getter for the work's expected bytes per 1e12 weight, clamped to [MIN_BANDWIDTH_RATE 60_000, MAX_BANDWIDTH_RATE 2_000_000_000]; unset (0) still fails closed in settlement. registerWork's signature is unchanged." },
      ],
      specs: [
        { label: "recognition & ownership", href: "docs/superpowers/specs/2026-07-21-recognition-and-ownership-design.md" },
        { label: "H5 credibility integrity design (cycle 2)", href: "docs/superpowers/specs/2026-07-26-h5-cycle2-credibility-integrity-design.md" },
      ],
      deps: ["identity"],
    },
    {
      id: "escrow",
      label: "CWEEscrow",
      layer: "chain",
      status: "done",
      tags: ["H1", "Phase 2·3"],
      desc: "Holds fingerprint-matched credit behind a challenge window instead of paying it out. A challenge opens an async jury dispute; the verdict reassigns or releases. The anti-fraud money spine.",
      files: ["chain/contracts/CWEEscrow.sol"],
      parts: [
        { label: "Commit → challenge → release", status: "done", desc: "Earliest-registration default; content-correlation required." },
        { label: "Async dispute flow", status: "done", desc: "Challenge opens a jury vote; resolveDispute applies the verdict." },
      ],
      specs: [{ label: "arbitration jury", href: "docs/superpowers/specs/2026-07-22-arbitration-jury-design.md" }],
      deps: ["registry", "jury", "payouts"],
    },
    {
      id: "jury",
      label: "CWEJury",
      layer: "chain",
      status: "done",
      tags: ["Phase 2·3"],
      desc: "A trusted committee that resolves contested ownership by majority vote — filing, voting, finalize — with earliest-registration as the tie/silence fallback. Its verdict moves the escrowed money.",
      files: ["chain/contracts/CWEJury.sol"],
      parts: [
        { label: "file → vote → finalize", status: "done", desc: "Allowlisted jurors, one vote each, permissionless tally after the window." },
        { label: "Credential-gated jurors", status: "done", desc: "H6: votes only with a valid juror credential (allowlist removed)." },
        { label: "Trustless staked court", status: "planned", desc: "Commit-reveal + slashing at the same IJury seam." },
      ],
      specs: [{ label: "arbitration jury", href: "docs/superpowers/specs/2026-07-22-arbitration-jury-design.md" }],
      deps: ["registry", "identity"],
    },
    {
      id: "payouts",
      label: "CWEPayouts",
      layer: "chain",
      status: "done",
      tags: ["Phase 1"],
      desc: "The payout ledger/pool: creators withdraw against the epoch's committed Merkle root, split-paid to their registered payees.",
      files: ["chain/contracts/CWEPayouts.sol"],
      parts: [],
      specs: [],
      deps: ["registry"],
    },
    {
      id: "tiers-consumption",
      label: "Tiers · Consumption",
      layer: "chain",
      status: "done",
      tags: ["Phase 1"],
      desc: "CWETiers (subscription intake → payout pool) and CWEConsumption (opaque usage-commitment intake, checked by the proof verifier).",
      files: ["chain/contracts/CWETiers.sol", "chain/contracts/CWEConsumption.sol"],
      parts: [
        { label: "Tier capability tokens", status: "planned", desc: "H7: decouple tier from the wallet address." },
        { label: "Epoch beacon", status: "planned", desc: "H8: replace the fixed 30-day window." },
      ],
      specs: [],
      deps: [],
    },
    {
      id: "verifier",
      label: "Proof verifier",
      layer: "chain",
      status: "done",
      tags: ["Phase 1", "H2"],
      desc: "IProofVerifier: the ZK seam. A real Groth16Verifier (BN254 pairing check over the circuit's Poseidon digest) is now the default deploy; AcceptAllVerifier is retained as the legacy verifier for the four pre-ZK demos and the player (dual-mode) without touching the consumption contract.",
      files: ["chain/contracts/IProofVerifier.sol", "chain/contracts/AcceptAllVerifier.sol", "chain/contracts/Groth16Verifier.sol"],
      parts: [
        { label: "Groth16Verifier (BN254 pairing)", status: "done", desc: "Real on-chain proof verification against the devnet verifying key; default deploy for make zk-demo." },
        { label: "AcceptAllVerifier", status: "seam", desc: "Retained as the legacy verifier so the four pre-ZK demos + player keep working (dual-mode)." },
        { label: "Blinding-aware verification", status: "planned", desc: "H2 cycle 2: verify against a manifest-signature/tier-eligibility circuit once those land." },
      ],
      specs: [{ label: "ZK usage proof reqs", href: "docs/specs/zk_usage_proof_requirements.md" }],
      deps: ["zk-circuits"],
    },
    {
      id: "epoch-beacon",
      label: "CWEEpochBeacon",
      layer: "chain",
      status: "done",
      tags: ["H2"],
      desc: "Publishes a per-epoch key K_epoch that CWEConsumption and off-chain provers read via keyFor to bind a ZK usage proof to a specific epoch.",
      files: ["chain/contracts/CWEEpochBeacon.sol"],
      parts: [
        { label: "keyFor / setKey", status: "done", desc: "Owner publishes (or overwrites) the epoch's key; provers and the verifier read it via keyFor." },
        { label: "Real randomness beacon (VRF/drand)", status: "planned", desc: "Today's key is owner-picked and non-random, carrying no unpredictability or liveness guarantee — the H8 / H2-cycle-2 graduation." },
      ],
      specs: [{ label: "epoch beacon spec", href: "docs/specs/epoch_beacon_specification.md" }],
      deps: [],
    },

    // ---- Devnet & CI ----------------------------------------------------
    {
      id: "ops",
      label: "Demos & CI",
      layer: "ops",
      status: "done",
      tags: ["Phase 1 →", "H2", "H5"],
      desc: "Nine self-contained, one-command Anvil demos, each an exit-criterion proof, all wired into CI alongside the Rust and contract suites.",
      files: ["ops/"],
      parts: [
        { label: "demo · hub · ownership", status: "done", desc: "Settlement, discovery, and the consented-split + escrow-challenge flows." },
        { label: "player · arbitration · antifraud", status: "done", desc: "Desktop pay-cycle, committee-overrides-fraudster, and fraud-is-capped." },
        { label: "identity", status: "done", desc: "H6: the credential lifecycle demo (attest / revoke / expiry / issuer-removal)." },
        { label: "zk-demo", status: "done", desc: "H2: honest usage proof pays; digest tampering and circuit row-splitting are both rejected on-chain/in-circuit." },
        { label: "bandwidth-demo", status: "done", desc: "H5: six acts — honest download pays in full; never-downloaded is a live strict loss; refetch amplification collapses to one credited chunk; a dust-weight claim is gutted by the epoch floor; an unset bandwidth rate fails closed; an uncredentialed node's receipts are rejected (bandwidth-e2e in CI)." },
      ],
      specs: [],
      deps: ["registry", "escrow", "jury", "settlement", "discovery-hub", "dapr", "identity", "verifier", "epoch-beacon", "zk-circuits", "storage-node", "receipt"],
    },
  ],

  // The roadmap as a flat timeline strip: phases and the hardening track.
  roadmap: [
    { id: "p1", kind: "phase", label: "Phase 1 — Music MVP", status: "done" },
    { id: "p2", kind: "phase", label: "Phase 2 — Video & News (Hub · Player · Jury)", status: "done" },
    { id: "p3", kind: "phase", label: "Phase 3 — Creator DMF", status: "planned" },
    { id: "p4", kind: "phase", label: "Phase 4 — Governance", status: "planned" },
    { id: "h1", kind: "harden", label: "H1 — Recognition & Ownership", status: "done" },
    { id: "h3", kind: "harden", label: "H3 — Full DAPR + anti-fraud", status: "done" },
    { id: "h6", kind: "harden", label: "H6 — Verifiable credentials / identity", status: "done" },
    { id: "h2", kind: "harden", label: "H2 — ZK usage proofs (cycle 1)", status: "done" },
    { id: "h5", kind: "harden", label: "H5 — Storage + real bandwidth receipts (cycle 1)", status: "done" },
    { id: "h5c2", kind: "harden", label: "H5 cycle 2 — credibility integrity: chunk dedup, delivery-gated credit, per-epoch evidence floor, on-chain RATE(W)", status: "done" },
    { id: "h4", kind: "harden", label: "H4 — Decentralised settlement", status: "planned" },
    { id: "hx", kind: "harden", label: "H7–H10 — tiers · epoch beacon · discovery v2 · security", status: "planned" },
  ],
};
