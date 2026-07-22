// Background service worker (dev-spec §1.2–1.3): the extension's brain.
//
// Responsibilities:
//   * initialise the Rust/WASM core (fingerprint, commitment, session accrual);
//   * on each play, fingerprint the source, resolve it via the static hub, apply
//     the price-cap policy, and start/stop accrual;
//   * on "Settle epoch", flush accrued usage into commitments and submit them to
//     the CWEConsumption contract, exporting the openings for the aggregator.
//
// The heavy logic lives in Rust (the WASM core) and in the shared libs; this file
// is the JavaScript glue the browser requires.

import init, { fingerprint, content_hash, commitment, WasmSession } from "../pkg/cwe_ext_core.js";
import { JsonRpcProvider, Wallet, Contract } from "ethers";
import { StaticHubClient, NetworkedHubClient } from "./hub.js";
import { allows } from "./policy.js";

// Minimal ABI for the one call the extension makes on-chain.
const CONSUMPTION_ABI = [
  "function submitConsumption(bytes32 tierId, bytes32[] workCommitments, bytes proof) external",
];

// Lazily-initialised singletons, set up by `ensureReady`.
let ready = null; // a Promise that resolves once WASM + hub are loaded
let hub = null; // StaticHubClient over assets/works.json
let session = null; // the WasmSession accrual store

/** Current wall-clock time in whole seconds (epoch anchor for the session). */
function nowSecs() {
  return Math.floor(Date.now() / 1000);
}

/** Initialise the WASM core, the hub manifest, and the session — exactly once. */
function ensureReady() {
  if (!ready) {
    ready = (async () => {
      // Load the WASM module packaged with the extension.
      await init({ module_or_path: chrome.runtime.getURL("cwe_ext_core_bg.wasm") });
      // Load the static works manifest that maps fingerprints to works.
      const manifest = await fetch(chrome.runtime.getURL("works.json")).then((r) => r.json());
      const staticClient = new StaticHubClient(manifest);
      // A configured hubUrl switches resolution to the live Discovery Hub, with the
      // static manifest as its fallback on a miss or network error.
      const stored2 = await chrome.storage.local.get("config");
      const hubUrl = stored2.config && stored2.config.hubUrl;
      hub = hubUrl ? new NetworkedHubClient(hubUrl, staticClient) : staticClient;
      // Restore a persisted session snapshot, or start a fresh one.
      const stored = await chrome.storage.local.get("sessionSnapshot");
      session = stored.sessionSnapshot
        ? WasmSession.restore(stored.sessionSnapshot)
        : new WasmSession(nowSecs());
    })();
  }
  return ready;
}

/** Persist the current session snapshot so it survives worker restarts. */
async function persistSession() {
  await chrome.storage.local.set({ sessionSnapshot: session.snapshot() });
}

/** Read the user configuration (RPC, key, contracts, tier, threshold). */
async function getConfig() {
  const cfg = await chrome.storage.local.get("config");
  return cfg.config || {};
}

/**
 * Handle a media element beginning to play: recognize (two-tier), apply policy,
 * and start accrual.
 *
 * The content script provides the media's raw bytes (for the Tier 1 content hash)
 * and decoded audio samples (for the Tier 2 perceptual fingerprint). Tier 1 —
 * a signed-content match — is authoritative and pays directly; Tier 2 — a
 * fingerprint match on unsigned content — is escrow-bound.
 */
async function handlePlay({ sessionId, contentBytes, samples, sampleRate }) {
  await ensureReady();

  // Tier 1 input: the exact content id (keccak256 of the content bytes).
  const contentId = contentBytes && contentBytes.length
    ? content_hash(new Uint8Array(contentBytes))
    : null;
  // Tier 2 input: the perceptual fingerprint of the decoded audio.
  const fp = samples && samples.length
    ? fingerprint(new Float32Array(samples), sampleRate || 44100)
    : null;

  // Recognize: the networked hub tries signed content first, then fingerprint;
  // the static fallback client only knows fingerprints.
  let work;
  if (typeof hub.recognize === "function") {
    work = await hub.recognize({ contentId, fingerprint: fp });
  } else if (fp) {
    const w = await hub.resolveFingerprint(fp);
    work = w ? { ...w, tier: "fingerprint" } : null;
  }
  // Unknown work: nothing to account or charge for.
  if (!work) return { ok: true, resolved: false };

  // Apply the user's price cap before accruing anything.
  const cfg = await getConfig();
  if (!allows(work.price_per_min, cfg.threshold)) {
    return { ok: true, block: true, reason: "Price cap exceeded" };
  }

  // Begin accruing time to this work.
  session.start(sessionId, work.work_id);
  // A fingerprint (Tier 2) recognition is escrow-bound: remember the work so the
  // settle flow marks it for escrow rather than a direct payout.
  if (work.tier === "fingerprint") {
    await recordEscrowWork(work.work_id);
  }
  await persistSession();
  return { ok: true, resolved: true, work_id: work.work_id, tier: work.tier };
}

/** Persist a fingerprint-recognized (escrow-bound) work id for the settle flow. */
async function recordEscrowWork(workId) {
  const { escrowWorks } = await chrome.storage.local.get("escrowWorks");
  const set = new Set(escrowWorks || []);
  set.add(workId);
  await chrome.storage.local.set({ escrowWorks: [...set] });
}

/** Handle progress: add elapsed seconds to the session's work. */
async function handleProgress({ sessionId, dt }) {
  await ensureReady();
  session.add_time(sessionId, dt);
  await persistSession();
  return { ok: true };
}

/** Handle stop: close the session (time already accrued). */
async function handleStop({ sessionId }) {
  await ensureReady();
  session.stop(sessionId);
  await persistSession();
  return { ok: true };
}

/**
 * Settle the epoch: flush accrued usage into commitments and submit them.
 *
 * Returns the openings so the operator can hand them to the aggregator (the
 * Phase 1 disclosure file) — the manual stand-in for a ZK proof.
 */
async function handleSettle() {
  await ensureReady();
  const cfg = await getConfig();

  // Drain the epoch's usage: [{ work_id, minutes, plays }, ...].
  const usage = JSON.parse(session.flush());
  if (usage.length === 0) return { ok: false, error: "no usage accrued this epoch" };

  // Build one commitment per work, remembering the openings for disclosure.
  const commitments = [];
  const openings = [];
  for (const u of usage) {
    // A fresh random 32-byte salt hides the minutes/plays and binds the commitment.
    const saltBytes = crypto.getRandomValues(new Uint8Array(32));
    const salt = "0x" + [...saltBytes].map((b) => b.toString(16).padStart(2, "0")).join("");
    commitments.push(commitment(u.work_id, u.minutes, u.plays, salt));
    openings.push({ work_id: u.work_id, minutes: u.minutes, plays: u.plays, salt });
  }

  // Persist the flushed (now empty) session and the openings for export.
  await persistSession();
  await chrome.storage.local.set({ lastOpenings: openings });

  // Submit the commitments on-chain via the configured signer.
  const provider = new JsonRpcProvider(cfg.rpcUrl);
  const signer = new Wallet(cfg.privateKey, provider);
  const consumption = new Contract(cfg.consumption, CONSUMPTION_ABI, signer);
  const tx = await consumption.submitConsumption(cfg.tierId, commitments, "0x");
  const receipt = await tx.wait();

  return { ok: true, txHash: receipt.hash, commitments, openings };
}

// Route messages from the content script and popup to their handlers. Returning
// `true` keeps the message channel open for the async response.
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  const routes = {
    play: handlePlay,
    progress: handleProgress,
    stop: handleStop,
    settle: handleSettle,
  };
  const handler = routes[message.type];
  if (!handler) return false;
  // Run the async handler and forward its result (or a clear error) back.
  handler(message)
    .then((result) => sendResponse(result))
    .catch((err) => sendResponse({ ok: false, error: String(err && err.message ? err.message : err) }));
  return true;
});
