// Popup logic: manage the price cap, trigger settlement, and export openings.
// Plain (no imports) so it needs no bundling; it talks to the background worker
// over chrome messaging and reads/writes chrome.storage directly.

// Cache the DOM nodes the handlers touch.
const thresholdInput = document.getElementById("threshold");
const status = document.getElementById("status");

/** Show a status line in the popup. */
function setStatus(text) {
  status.textContent = text;
}

// Load the saved cap into the input on open.
chrome.storage.local.get("config").then(({ config }) => {
  thresholdInput.value = (config && config.threshold) || 0;
});

// Save the cap back into the config object (merging with existing fields).
document.getElementById("save").addEventListener("click", async () => {
  const { config } = await chrome.storage.local.get("config");
  const next = { ...(config || {}), threshold: Number(thresholdInput.value) || 0 };
  await chrome.storage.local.set({ config: next });
  setStatus("Price cap saved.");
});

// Ask the background worker to settle the epoch and report the outcome.
document.getElementById("settle").addEventListener("click", async () => {
  setStatus("Settling…");
  const resp = await chrome.runtime.sendMessage({ type: "settle" });
  if (resp && resp.ok) {
    setStatus(`Submitted ${resp.commitments.length} commitment(s).\nTx: ${resp.txHash}`);
  } else {
    setStatus(`Settle failed: ${(resp && resp.error) || "unknown error"}`);
  }
});

// Download the last settlement's openings as the disclosure file the aggregator needs.
document.getElementById("export").addEventListener("click", async () => {
  const { lastOpenings } = await chrome.storage.local.get("lastOpenings");
  if (!lastOpenings || lastOpenings.length === 0) {
    setStatus("No openings to export yet — settle first.");
    return;
  }
  // Build the disclosure shape { users: { <address>: [openings] } } is assembled
  // by the operator across users; here we export just this user's openings.
  const blob = new Blob([JSON.stringify(lastOpenings, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  // Trigger a download via a transient anchor.
  const a = document.createElement("a");
  a.href = url;
  a.download = "openings.json";
  a.click();
  URL.revokeObjectURL(url);
  setStatus("Exported openings.json");
});
