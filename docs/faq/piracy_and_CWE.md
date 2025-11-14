<!-- File: docs/faq/is_cwe_a_piracy_haven.md -->

# FAQ: Is CWE a Piracy Haven? Why It Is Actually Anti-Piracy

**Version:** Draft v1.0
**Status:** Public-Facing FAQ Article

---

## 1. Short Answer

No — the Clean Web Economy (CWE) is **the first media ecosystem in history where piracy cannot deprive creators of income**. Even if someone watches a pirated copy, CWE automatically redirects the revenue to the legitimate creator via fingerprint matching.

This means:

* Piracy cannot steal revenue.
* Piracy cannot suppress creators.
* Piracy cannot hijack attribution.

CWE is therefore **inherently anti-piracy**, without using DRM or surveillance.

---

## 2. Why People Think CWE Might Enable Piracy

At first glance, CWE looks dangerously open:

* Encrypted chunks are stored by volunteers
* Clients are open-source
* Users can view content locally
* No DRM or device lock-in exists

On traditional platforms, these things usually imply easier piracy.
But in CWE, the economics are **reversed**.

---

## 3. The Core Anti-Piracy Mechanism: Fingerprint Redirection

Every work in CWE has a **creator-signed manifest** and a **perceptual fingerprint**.

If a user views **any file** — signed, unsigned, downloaded, shared, re-encoded, recompressed — the client compares its fingerprint against the global index.

If the file matches a known work, usage credit is sent to the rightful creator.

### This means:

* **Watching a pirated file still pays the creator.**
* **Uploading stolen content earns you nothing.**
* **Re-encoding or disguising the file does not avoid attribution.**

This is fundamentally impossible on current platforms.

---

## 4. How Usage Redirection Works

1. User watches a file.
2. Client computes its perceptual fingerprint.
3. Client submits **usage commitment** to the network.
4. ZK proofs ensure usage is real.
5. Aggregation nodes map the usage to the canonical creator manifest.
6. On-chain settlement pays the correct creator.

It does not matter where the file was downloaded from.
It does not matter if the file was stolen.
The client still rewards the legitimate source.

---

## 5. What If Someone Uploads Stolen Content to CWE?

Nothing happens — they simply **cannot monetize it**.

All payouts are based on:

* Manifest signatures
* Fingerprints
* Immutable collaboration splits

Uploading an unauthorized copy results in:

* Zero revenue for the thief
* Full revenue for the legitimate creator
* Optional flagging in discovery for plagiarism

This closes the economic incentive for piracy-based monetization.

---

## 6. What If Someone Re-Encodes a Video to Manipulate the Fingerprint?

The fingerprinting system is **multi-modal**:

* Audio spectral features
* Video frame embeddings
* Subtitle + dialogue embeddings
* Temporal structure signatures

Minor modifications do not break attribution.
Major modifications produce *derivative* fingerprints but still link back via near-duplicate matching.

A true fingerprint collision attack requires:

* Deliberate machine-learning adversarial examples
* High expertise
* Significant compute

Even then, attribution disputes can be resolved via DAO arbitration.

---

## 7. Does CWE Allow Pirate Movie/MP3 Sites?

No.

CWE’s storage nodes:

* Store encrypted shards only
* Cannot categorize content
* Cannot serve plaintext files

Users cannot simply "browse" a directory of works.
The only way to retrieve content is via:

* A signed manifest
* A registered fingerprint

Unsigned pirate repositories cannot integrate with the ecosystem.

---

## 8. Does CWE Track Users to Stop Piracy?

No.

CWE bans:

* Device fingerprinting
* DRM
* Identifying logs
* Surveillance-based enforcement
* Trusted execution or secure enclaves

The system stops piracy via **cryptography**, not coercion.

---

## 9. What About Streaming New Releases?

Creators can choose:

* Day-one release
* Delayed window
* Premium tiers
* Direct purchase bundles

CWE does not force anyone to publish early.
Creators retain full control of availability.

---

## 10. Why Studios and Labels Should Prefer CWE

CWE solves the three biggest problems in digital media:

### **1. Piracy No Longer Hurts Revenue**

Watching a pirated file still pays the rightful creator.

### **2. No Middlemen Extract Revenue**

There is no ad platform, no algorithmic manipulation, no opaque payout rules.

### **3. No User Tracking = No Legal Liability**

CWE clients and nodes do not collect personal data.
No GDPR/CCPA compliance burdens.

This creates trust on the legal and business sides simultaneously.

---

## 11. What If Governments Claim CWE Enables Piracy?

The response is simple:

> CWE is the **only** media ecosystem where piracy cannot reduce creator revenue.

This satisfies or exceeds the goals of:

* DMCA
* EU copyright directives
* WIPO treaties

Without requiring DRM or surveillance.

---

## 12. Summary

CWE is not a piracy haven.
It is the first open media ecosystem where:

* Creators always get paid
* Stolen content cannot earn money
* Viewing pirated files still benefits creators
* No DRM or surveillance is needed
* All attribution is cryptographically enforced

This is a new paradigm: **piracy-proof economics without user harm**.

CWE is designed not to punish users, but to eliminate the economic incentives behind piracy — permanently.

