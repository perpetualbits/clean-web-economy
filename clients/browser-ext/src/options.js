// Options logic: load and save the devnet connection settings into the shared
// `config` object in chrome.storage. Plain script (no imports, no bundling).

// The config fields this page manages (the price cap lives in the popup).
const FIELDS = ["rpcUrl", "consumption", "tierId", "privateKey"];

// Populate the inputs from stored config on load.
chrome.storage.local.get("config").then(({ config }) => {
  const cfg = config || {};
  for (const field of FIELDS) {
    const el = document.getElementById(field);
    if (el) el.value = cfg[field] || "";
  }
});

// Merge the edited fields back into config, preserving unrelated keys (threshold).
document.getElementById("save").addEventListener("click", async () => {
  const { config } = await chrome.storage.local.get("config");
  const next = { ...(config || {}) };
  for (const field of FIELDS) {
    next[field] = document.getElementById(field).value.trim();
  }
  await chrome.storage.local.set({ config: next });
  document.getElementById("status").textContent = "Settings saved.";
});
