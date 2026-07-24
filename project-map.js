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
    updated: "2026-07-24",
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
        { label: "Signed manifest ingest", status: "done", desc: "Only the on-chain registrant can publish; price/region re-verified." },
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
      tags: ["Phase 1", "H1", "H3"],
      desc: "Closes an epoch: reads on-chain usage submissions, opens their commitments from the disclosure, runs DAPR, commits a Merkle root, and routes signed usage to direct payout vs. fingerprint usage to escrow.",
      files: ["services/settlement/"],
      parts: [
        { label: "DAPR + Merkle root", status: "done", desc: "Per-work credits → committable root + withdrawal proofs." },
        { label: "Direct vs. escrow routing", status: "done", desc: "Signed → CWEPayouts; fingerprint → CWEEscrow." },
        { label: "Decentralised settlement", status: "planned", desc: "H4: rollup / multi-aggregator instead of one trusted aggregator." },
      ],
      specs: [{ label: "DAPR protocol", href: "docs/specs/DAPR_usage_aggregation_protocol.md" }],
      deps: ["dapr", "wallet-zk", "registry", "payouts", "escrow"],
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
      tags: ["Phase 1", "H3"],
      desc: "Keccak usage commitments (now binding work + minutes + plays), the epoch session store shared by both clients, and the ZK seam. The trust primitive under all usage reporting.",
      files: ["libs/wallet-zk/"],
      parts: [
        { label: "Usage commitments", status: "done", desc: "Hiding commitment over (work, minutes, plays, salt)." },
        { label: "Session store", status: "done", desc: "Per-work time + play counting; snapshot/restore." },
        { label: "Real ZK proofs", status: "seam", desc: "H2: replace the disclosure-file stand-in with circuits behind IProofVerifier." },
      ],
      specs: [{ label: "ZK usage proof reqs", href: "docs/specs/zk_usage_proof_requirements.md" }],
      deps: [],
    },
    {
      id: "dapr",
      label: "Payout math (DAPR)",
      layer: "core",
      status: "done",
      tags: ["Phase 1", "H3"],
      desc: "The fair-split brain (cwe-dapr): user-centric split with diminishing returns on repeats, a bandwidth-credibility discount, and a reputation signal — deterministic integer math, fee-conserving.",
      files: ["sims/"],
      parts: [
        { label: "Diminishing returns", status: "done", desc: "The j-th replay counts for 1/j; caps replay inflation." },
        { label: "Bandwidth discount", status: "done", desc: "Neutral input now; a fake (no-data) play pays out less." },
        { label: "Reputation signal", status: "done", desc: "Distinct-listener breadth for discovery ranking." },
        { label: "Real bandwidth receipts", status: "seam", desc: "H5 supplies the cryptographic 'data actually moved' proof." },
      ],
      specs: [{ label: "full DAPR + anti-fraud", href: "docs/superpowers/specs/2026-07-22-full-dapr-antifraud-design.md" }],
      deps: [],
    },

    // ---- On-chain contracts --------------------------------------------
    {
      id: "identity",
      label: "CWEIdentity (credentials)",
      layer: "chain",
      status: "done",
      tags: ["H6"],
      desc: "Replaces the owner-managed 'verified creator' and juror allowlists with revocable, expiring verifiable credentials: a rotatable issuer set attests; the registry and jury verify. Removing an issuer invalidates all their credentials.",
      files: ["chain/contracts/CWEIdentity.sol", "chain/contracts/interfaces/ICWEIdentity.sol"],
      parts: [
        { label: "Issue / verify / expire / revoke", status: "done", desc: "isValid = exists ∧ ¬revoked ∧ ¬expired ∧ issuer still trusted." },
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
      tags: ["Phase 1", "H1"],
      desc: "The work registry: content_id, multi-party consent (each payee signs their share), and a registration timestamp used as the dispute priority key.",
      files: ["chain/contracts/CWERegistry.sol"],
      parts: [
        { label: "content_id + timestamp", status: "done", desc: "Authoritative signed ownership + earliest-registration key." },
        { label: "Multi-party consent", status: "done", desc: "Every payee EIP-191 signs their exact split." },
        { label: "Credential-gated", status: "done", desc: "H6: registers only with a valid verified-creator credential (allowlist removed)." },
      ],
      specs: [{ label: "recognition & ownership", href: "docs/superpowers/specs/2026-07-21-recognition-and-ownership-design.md" }],
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
      status: "seam",
      tags: ["Phase 1"],
      desc: "IProofVerifier + AcceptAllVerifier: the ZK seam. Phase 1 accepts every proof; H2 drops in real circuit verification without touching the consumption contract.",
      files: ["chain/contracts/IProofVerifier.sol", "chain/contracts/AcceptAllVerifier.sol"],
      parts: [{ label: "Real ZK verifier", status: "planned", desc: "H2 — the privacy backbone." }],
      specs: [{ label: "ZK usage proof reqs", href: "docs/specs/zk_usage_proof_requirements.md" }],
      deps: [],
    },

    // ---- Devnet & CI ----------------------------------------------------
    {
      id: "ops",
      label: "Demos & CI",
      layer: "ops",
      status: "done",
      tags: ["Phase 1 →"],
      desc: "Seven self-contained, one-command Anvil demos, each an exit-criterion proof, all wired into CI alongside the Rust and contract suites.",
      files: ["ops/"],
      parts: [
        { label: "demo · hub · ownership", status: "done", desc: "Settlement, discovery, and the consented-split + escrow-challenge flows." },
        { label: "player · arbitration · antifraud", status: "done", desc: "Desktop pay-cycle, committee-overrides-fraudster, and fraud-is-capped." },
        { label: "identity", status: "done", desc: "H6: the credential lifecycle demo (attest / revoke / expiry / issuer-removal)." },
      ],
      specs: [],
      deps: ["registry", "escrow", "jury", "settlement", "discovery-hub", "dapr"],
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
    { id: "h2", kind: "harden", label: "H2 — ZK usage proofs", status: "planned" },
    { id: "h5", kind: "harden", label: "H5 — Storage + real bandwidth receipts", status: "planned" },
    { id: "h4", kind: "harden", label: "H4 — Decentralised settlement", status: "planned" },
    { id: "hx", kind: "harden", label: "H7–H10 — tiers · epoch beacon · discovery v2 · security", status: "planned" },
  ],
};
