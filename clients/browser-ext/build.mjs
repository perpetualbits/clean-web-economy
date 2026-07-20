// Build the extension into `dist/` — a directory loadable as an unpacked MV3
// extension. The background worker is bundled (it imports the WASM glue, ethers,
// and the hub/policy modules); everything else is a plain file that is copied.
//
// Prerequisite: `npm run build:wasm` has produced `pkg/` via wasm-pack.

import { build } from "esbuild";
import { cpSync, mkdirSync, rmSync, existsSync } from "node:fs";

const DIST = "dist";

// Start from a clean output directory so stale files never linger.
rmSync(DIST, { recursive: true, force: true });
mkdirSync(DIST, { recursive: true });

// Fail early with a clear message if the WASM build has not run yet.
if (!existsSync("pkg/cwe_ext_core_bg.wasm")) {
  console.error("missing pkg/ — run `npm run build:wasm` first");
  process.exit(1);
}

// Bundle the service worker and its dependency graph into a single ES module.
await build({
  entryPoints: ["src/background.js"],
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  outfile: `${DIST}/background.js`,
});

// Copy the files that are loaded as-is (no bundling needed).
const copies = [
  ["src/content-script.js", "content-script.js"],
  ["src/popup.html", "popup.html"],
  ["src/popup.js", "popup.js"],
  ["src/options.html", "options.html"],
  ["src/options.js", "options.js"],
  ["manifest.json", "manifest.json"],
  ["assets/works.json", "works.json"],
  ["pkg/cwe_ext_core_bg.wasm", "cwe_ext_core_bg.wasm"],
];
for (const [from, to] of copies) {
  cpSync(from, `${DIST}/${to}`);
}

console.log(`built extension -> ${DIST}/`);
