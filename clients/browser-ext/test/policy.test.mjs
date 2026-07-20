// Unit tests for the price-cap policy (run with `node --test`).
import { test } from "node:test";
import assert from "node:assert/strict";
import { allows } from "../src/policy.js";

test("no cap (0 or negative) allows any price", () => {
  assert.equal(allows(999, 0), true);
  assert.equal(allows(999, -5), true);
});

test("price at or below the cap is allowed", () => {
  assert.equal(allows(100, 100), true);
  assert.equal(allows(50, 100), true);
});

test("price above the cap is blocked", () => {
  assert.equal(allows(101, 100), false);
});
