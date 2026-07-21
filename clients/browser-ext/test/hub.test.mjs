// Unit tests for the static hub resolver (run with `node --test`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { StaticHubClient, NetworkedHubClient } from "../src/hub.js";

const MANIFEST = {
  "fp:aaaa": { work_id: "0x01", price_per_min: 100, region_factor: 1000000 },
};

test("resolves a known fingerprint to its work", () => {
  const hub = new StaticHubClient(MANIFEST);
  const work = hub.resolveFingerprint("fp:aaaa");
  assert.equal(work.work_id, "0x01");
  assert.equal(work.price_per_min, 100);
});

test("returns null for an unknown fingerprint", () => {
  const hub = new StaticHubClient(MANIFEST);
  assert.equal(hub.resolveFingerprint("fp:unknown"), null);
});

test("an empty/missing manifest resolves nothing", () => {
  const hub = new StaticHubClient();
  assert.equal(hub.resolveFingerprint("fp:aaaa"), null);
});

test("networked client resolves a fingerprint via the hub (candidate shape)", async () => {
  const fakeFetch = async (url) => ({
    ok: url.endsWith("/resolve/fingerprint/fp:aaaa"),
    json: async () => ({ candidate: { work_id: "0x01", price_per_min: 100, work_type: "audio" }, similarity: 0.97 }),
  });
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.resolveFingerprint("fp:aaaa");
  assert.equal(work.work_id, "0x01");
  assert.equal(work.price_per_min, 100);
});

test("networked client resolves content id (Tier 1, authoritative)", async () => {
  const fakeFetch = async (url) => ({
    ok: url.endsWith("/resolve/content/0xcontent"),
    json: async () => ({ work_id: "0x09", price_per_min: 50, work_type: "audio" }),
  });
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.resolveContent("0xcontent");
  assert.equal(work.work_id, "0x09");
});

test("networked client falls back to the static client on fingerprint miss", async () => {
  const fakeFetch = async () => ({ ok: false });
  const fallback = new StaticHubClient({ "fp:bbbb": { work_id: "0x02", price_per_min: 5, region_factor: 1 } });
  const hub = new NetworkedHubClient("http://hub.test", fallback, fakeFetch);
  const work = await hub.resolveFingerprint("fp:bbbb");
  assert.equal(work.work_id, "0x02");
});

test("recognize prefers Tier 1 signed content over Tier 2 fingerprint", async () => {
  // Content resolves (signed); fingerprint would also resolve, but signed wins.
  const fakeFetch = async (url) => {
    if (url.includes("/resolve/content/")) {
      return { ok: true, json: async () => ({ work_id: "0xSIGNED", price_per_min: 10, work_type: "audio" }) };
    }
    return { ok: true, json: async () => ({ candidate: { work_id: "0xFP", price_per_min: 10 }, similarity: 0.99 }) };
  };
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.recognize({ contentId: "0xc", fingerprint: "fp:aaaa" });
  assert.equal(work.work_id, "0xSIGNED");
  assert.equal(work.tier, "signed");
});

test("recognize falls back to Tier 2 fingerprint (escrow-bound) when unsigned", async () => {
  // Content misses (unsigned); fingerprint matches -> tier "fingerprint".
  const fakeFetch = async (url) => {
    if (url.includes("/resolve/content/")) return { ok: false };
    return { ok: true, json: async () => ({ candidate: { work_id: "0xFP", price_per_min: 10 }, similarity: 0.99 }) };
  };
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.recognize({ contentId: "0xc", fingerprint: "fp:aaaa" });
  assert.equal(work.work_id, "0xFP");
  assert.equal(work.tier, "fingerprint");
});
