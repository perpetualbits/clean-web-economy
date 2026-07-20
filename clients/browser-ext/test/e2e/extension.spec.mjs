// End-to-end test: load the built extension in Chromium and verify the
// resolve → policy → block path on a real page with an <audio> element.
//
// It serves a tiny page over http://127.0.0.1 (so the content script's host
// match applies), seeds the works manifest so the audio resolves to a work
// priced ABOVE the configured cap, plays the audio, and asserts the content
// script pauses it and shows the block overlay.
//
// Prerequisite: `npm run build` (the test loads `dist/`). Run: `npm run test:e2e`.

import { test, expect, chromium } from "@playwright/test";
import { fileURLToPath } from "node:url";
import path from "node:path";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";

const dir = path.dirname(fileURLToPath(import.meta.url));
const DIST = path.resolve(dir, "../../dist");
const PORT = 3999;
const BASE = `http://127.0.0.1:${PORT}`;
const AUDIO_PATH = "/tone.wav";
const AUDIO_URL = `${BASE}${AUDIO_PATH}`;

// The extension fingerprints a work by its source URL bytes (SHA-256, `fp:` hex).
const FP = "fp:" + crypto.createHash("sha256").update(AUDIO_URL).digest("hex");

/** Build a tiny valid silent WAV (8 kHz mono, ~0.3 s) so the element can play. */
function makeWav() {
  const sampleRate = 8000;
  const samples = Math.floor(sampleRate * 0.3);
  const dataLen = samples; // 8-bit mono => one byte per sample
  const buf = Buffer.alloc(44 + dataLen);
  buf.write("RIFF", 0);
  buf.writeUInt32LE(36 + dataLen, 4);
  buf.write("WAVE", 8);
  buf.write("fmt ", 12);
  buf.writeUInt32LE(16, 16); // fmt chunk size
  buf.writeUInt16LE(1, 20); // PCM
  buf.writeUInt16LE(1, 22); // mono
  buf.writeUInt32LE(sampleRate, 24);
  buf.writeUInt32LE(sampleRate, 28); // byte rate
  buf.writeUInt16LE(1, 32); // block align
  buf.writeUInt16LE(8, 34); // bits per sample
  buf.write("data", 36);
  buf.writeUInt32LE(dataLen, 40);
  buf.fill(128, 44); // 8-bit silence is centred at 128
  return buf;
}

let server;
let context;

test.beforeAll(async () => {
  // Seed the works manifest so the audio URL resolves to a work priced ABOVE cap.
  fs.writeFileSync(
    path.join(DIST, "works.json"),
    JSON.stringify({ [FP]: { work_id: "0x" + "11".repeat(32), price_per_min: 100, region_factor: 1000000 } })
  );

  // Serve the test page and the WAV.
  const wav = makeWav();
  server = http.createServer((req, res) => {
    if (req.url === AUDIO_PATH) {
      res.setHeader("content-type", "audio/wav");
      res.end(wav);
    } else {
      res.setHeader("content-type", "text/html");
      res.end('<!doctype html><audio id="player" src="tone.wav"></audio>');
    }
  });
  await new Promise((resolve) => server.listen(PORT, resolve));

  // Launch Chromium with the unpacked extension loaded. The full Chromium build
  // in new-headless mode (`channel: "chromium"`) is required — the default
  // headless-shell cannot load extensions.
  context = await chromium.launchPersistentContext("", {
    channel: "chromium",
    headless: true,
    args: [
      `--disable-extensions-except=${DIST}`,
      `--load-extension=${DIST}`,
      "--autoplay-policy=no-user-gesture-required",
    ],
  });
});

test.afterAll(async () => {
  await context?.close();
  await new Promise((resolve) => server?.close(resolve));
});

test("over-cap work is blocked and shows the overlay", async () => {
  // Get the extension's service worker to configure a cap below the work price.
  let [sw] = context.serviceWorkers();
  if (!sw) sw = await context.waitForEvent("serviceworker");
  await sw.evaluate(() => chrome.storage.local.set({ config: { threshold: 50 } }));

  // Open the page and start playback.
  const page = await context.newPage();
  await page.goto(`${BASE}/`);
  await page.evaluate(() => document.getElementById("player").play().catch(() => {}));

  // The content script should surface the block overlay.
  await expect(page.getByText(/CWE:/)).toBeVisible({ timeout: 8000 });
});
