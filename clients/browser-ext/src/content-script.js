// Content script (dev-spec §1.2): watches media elements and reports playback.
//
// It stays deliberately thin — no fingerprinting or accounting here. It observes
// <audio>/<video> elements, and on play/progress/stop sends a message to the
// background worker, which does the fingerprinting, resolution, policy, and
// accrual. If the background says a work is over the user's price cap, this
// script pauses the element and shows a block overlay.
//
// Phase 1 identifies a work by its media source URL (the background hashes it).
// Tapping decoded audio via WebAudio is the richer path but is CORS-limited on
// third-party media; the URL is deterministic and demo-friendly. See the plan's
// WP6 risk note.

(function () {
  // Per-element session ids, so progress/stop refer to the same session as play.
  const sessions = new WeakMap();
  // Last reported playback position per element, to compute elapsed deltas.
  const lastTime = new WeakMap();

  /** Send a message to the background worker, ignoring disconnected-port errors. */
  function notify(message) {
    try {
      return chrome.runtime.sendMessage(message);
    } catch (_e) {
      // The worker may be asleep or the context invalidated; nothing to do.
      return Promise.resolve(undefined);
    }
  }

  /** Handle a media element beginning playback. */
  async function onPlay(el) {
    // A fresh session id ties together this play's progress and stop events.
    const sessionId = crypto.randomUUID();
    sessions.set(el, sessionId);
    lastTime.set(el, el.currentTime || 0);

    // The background resolves the source to a work, applies policy, and starts accrual.
    const src = el.currentSrc || el.src || "";
    const resp = await notify({ type: "play", sessionId, src });
    // If the work is over the user's cap, stop it and show why.
    if (resp && resp.block) {
      el.pause();
      showBlockOverlay(el, resp.reason || "Price cap exceeded");
    }
  }

  /** Handle a playback position update: report the elapsed seconds since last. */
  function onProgress(el) {
    const sessionId = sessions.get(el);
    if (!sessionId) return;
    const now = el.currentTime || 0;
    const prev = lastTime.get(el) || 0;
    // Only report forward progress (ignore seeks backwards and pauses).
    const dt = now - prev;
    lastTime.set(el, now);
    if (dt > 0 && dt < 60) {
      // Round to whole seconds; the accrual store works in seconds.
      notify({ type: "progress", sessionId, dt: Math.round(dt) });
    }
  }

  /** Handle a media element stopping (pause or ended). */
  function onStop(el) {
    const sessionId = sessions.get(el);
    if (!sessionId) return;
    notify({ type: "stop", sessionId });
    sessions.delete(el);
  }

  /** Overlay a small "blocked" banner on top of a media element. */
  function showBlockOverlay(el, reason) {
    const banner = document.createElement("div");
    banner.textContent = `CWE: ${reason}`;
    // Inline styles keep the overlay self-contained (no external CSS to load).
    banner.style.cssText =
      "position:fixed;top:8px;right:8px;z-index:2147483647;background:#b00020;" +
      "color:#fff;font:13px sans-serif;padding:6px 10px;border-radius:4px;";
    document.body.appendChild(banner);
    // Auto-dismiss so the page is not permanently decorated.
    setTimeout(() => banner.remove(), 4000);
  }

  /** Wire the playback listeners onto a media element exactly once. */
  function attach(el) {
    if (el.__cweAttached) return; // idempotent: never double-bind
    el.__cweAttached = true;
    el.addEventListener("play", () => onPlay(el));
    el.addEventListener("timeupdate", () => onProgress(el));
    el.addEventListener("pause", () => onStop(el));
    el.addEventListener("ended", () => onStop(el));
  }

  /** Attach to all current media elements. */
  function scan() {
    document.querySelectorAll("audio, video").forEach(attach);
  }

  // Bind existing elements now, and watch for elements added later (SPA pages).
  scan();
  const observer = new MutationObserver(scan);
  observer.observe(document.documentElement, { childList: true, subtree: true });
})();
