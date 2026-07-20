// Discovery-hub client (dev-spec §1.2, plan decision D4).
//
// A fingerprint identifies *what* is playing; the hub turns that into *who to pay*
// and *how much* — the work id, price per minute, and region factor. In Phase 1
// there is no Discovery Hub service, so resolution is a static manifest shipped
// with the extension (`assets/works.json`). The `HubClient` shape is preserved so
// a networked resolver can replace it in Phase 2 without touching callers.

/**
 * Resolves fingerprints against a static, in-memory works manifest.
 */
export class StaticHubClient {
  /**
   * @param {Object<string, {work_id: string, price_per_min: number, region_factor: number}>} manifest
   *   Map from `fp:<hex>` fingerprint to the work's payout metadata.
   */
  constructor(manifest) {
    // Default to an empty manifest so a missing file degrades to "nothing resolves".
    this.manifest = manifest || {};
  }

  /**
   * Resolve a fingerprint to its work metadata.
   *
   * @param {string} fingerprint The `fp:<hex>` identifier from the core.
   * @returns {?{work_id: string, price_per_min: number, region_factor: number}}
   *   The work metadata, or null if the fingerprint is unknown.
   */
  resolveFingerprint(fingerprint) {
    // A plain lookup; unknown works return null so the caller can ignore them.
    return this.manifest[fingerprint] || null;
  }
}

/**
 * Resolves fingerprints against a live Discovery Hub, falling back to a static
 * client on a miss or network error. `fetchImpl` is injectable for testing.
 */
export class NetworkedHubClient {
  /**
   * @param {string} hubUrl Base URL of the Discovery Hub.
   * @param {?StaticHubClient} fallback Used when the hub has no answer.
   * @param {typeof fetch} [fetchImpl] Fetch implementation (defaults to global fetch).
   */
  constructor(hubUrl, fallback, fetchImpl) {
    this.hubUrl = hubUrl.replace(/\/$/, "");
    this.fallback = fallback;
    this.fetchImpl = fetchImpl || fetch;
  }

  /**
   * Resolve a fingerprint via the hub, then the fallback.
   * @param {string} fingerprint The `fp:<hex>` identifier.
   * @returns {Promise<?object>} Work metadata or null.
   */
  async resolveFingerprint(fingerprint) {
    try {
      // The fingerprint's `fp:<hex>` form is used verbatim in the path (matches the hub's route).
      const resp = await this.fetchImpl(`${this.hubUrl}/resolve/${fingerprint}`);
      if (resp.ok) {
        // The hub returns {work_id, price_per_min, region, work_type}.
        return await resp.json();
      }
    } catch (_e) {
      // Network failure: fall through to the static fallback.
    }
    return this.fallback ? this.fallback.resolveFingerprint(fingerprint) : null;
  }
}
