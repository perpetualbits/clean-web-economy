// Unit tests for the static hub resolver (run with `node --test`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { StaticHubClient } from "../src/hub.js";

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
