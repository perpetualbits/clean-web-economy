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

test("networked client resolves via fetch and maps the response", async () => {
  const fakeFetch = async (url) => ({
    ok: url.endsWith("/resolve/fp:aaaa"),
    json: async () => ({ work_id: "0x01", price_per_min: 100, work_type: "audio" }),
  });
  const hub = new NetworkedHubClient("http://hub.test", null, fakeFetch);
  const work = await hub.resolveFingerprint("fp:aaaa");
  assert.equal(work.work_id, "0x01");
  assert.equal(work.price_per_min, 100);
});

test("networked client falls back to the static client on miss", async () => {
  const fakeFetch = async () => ({ ok: false });
  const fallback = new StaticHubClient({ "fp:bbbb": { work_id: "0x02", price_per_min: 5, region_factor: 1 } });
  const hub = new NetworkedHubClient("http://hub.test", fallback, fakeFetch);
  const work = await hub.resolveFingerprint("fp:bbbb");
  assert.equal(work.work_id, "0x02");
});
