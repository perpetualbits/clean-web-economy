// Price-threshold policy engine (dev-spec §1.2).
//
// The user sets a maximum price-per-minute they are willing to consume. Before a
// work starts accruing, the background worker checks it against this cap; works
// priced above the cap are blocked. Keeping this as a pure function makes it
// trivial to unit test and impossible to couple to the DOM or chrome APIs.

/**
 * Decide whether a work priced at `pricePerMin` may play under the user's cap.
 *
 * @param {number} pricePerMin   The work's price per minute (ppm units).
 * @param {number} thresholdPerMin The user's cap; <= 0 means "no cap".
 * @returns {boolean} true if playback is allowed.
 */
export function allows(pricePerMin, thresholdPerMin) {
  // A non-positive threshold disables the cap entirely — allow everything.
  if (!thresholdPerMin || thresholdPerMin <= 0) return true;
  // Otherwise the work is allowed only if it costs at or below the cap.
  return pricePerMin <= thresholdPerMin;
}
