export async function compute(samples: Float32Array): Promise<string> {
  // TODO: replace with real perceptual fingerprint
  return crypto.randomUUID();
}
