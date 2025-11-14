<!-- File: docs/faq/digital_albums_ownership_resale_inheritance.md -->

# FAQ: Digital Albums — Ownership, Resale, and Inheritance

**Version:** Draft v1.0
**Status:** Public-Facing FAQ Article

---

## 1. Short Answer

Yes. In the Clean Web Economy (CWE), **digital albums can be owned, resold, gifted, and inherited**, without DRM and without any centralized control. This works through cryptographic **rights tokens** bound to a creator-signed manifest, not through hardware locks or proprietary platforms.

CWE enables:

* **Personal ownership** of digital works
* **Optional resale** if the creator allows it
* **Gifting** to other users
* **Full inheritance** of a digital library and NFT-based collectibles

All of this is possible **without** DRM, device attestation, or surveillance.

---

## 2. Why CWE Can Offer Real Digital Ownership

Traditional platforms tie purchases to:

* A central store (e.g., iTunes)
* A specific account
* Often, a specific device

This is *not* ownership — it is a license controlled by the vendor.

CWE does it differently.

When you buy an album in CWE, you receive a **Transferable Rights Token (TRT)**:

```
Token = {
  work_id,
  creator_signature,
  rights_class,    // transferable, non-transferable, resale-royalty, etc.
  owner,           // DID of user
  extras,          // optional NFT artwork, message, autograph
}
```

This token is:

* Stored on the CWE L2 blockchain
* Verifiable independently
* Not tied to devices, apps, or platforms
* Not linked to your identity (unless you choose to)

Because the token is **cryptographically signed** and lives on a decentralized ledger, no service can revoke it or make it inaccessible.

---

## 3. Can You Resell a Digital Album?

**Yes, IF the creator chooses to allow it.**

Creators define allowed rights in the content manifest:

* `non_transferable` (like Bandcamp downloads)
* `transferable` (like vinyl or CDs)
* `transferable_with_resale_royalty` (like an NFT that pays 5% to the artist)

If resale is permitted, the transfer is simply:

```
transfer(token_id, new_owner)
```

No intermediaries needed.
No DRM preventing access.
No account dependency.

This mirrors the physical world: you can resell your CD, but only its owner can access it.

---

## 4. Can You Inherit a Digital Library?

**Yes. Fully.**

Rights tokens are cryptographic property.
They can be:

* Stored in a hardware or software wallet
* Exported to heirs
* Placed under a legal will
* Escrowed by executors if needed

Inheritance works exactly like inheriting:

* Cryptocurrencies
* Domain names
* Precious digital assets

Nothing in CWE’s design ties tokens to personal identity or hardware, so inheritance is natural.

---

## 5. What About NFTs and Collectibles?

Creators may optionally attach:

* Autographs
* Artwork
* Personal notes
* Limited-edition covers
* Audio stems
* Bonus tracks

These extras are stored **off-chain** in encrypted storage, referenced by the rights token.

The token proves:

* Authenticity
* Uniqueness
* Ownership

And can be resold or inherited in exactly the same way.

---

## 6. Does This Break Copyright Law?

No — it actually **aligns better** with copyright than most digital platforms.

In traditional digital stores, resale is banned because files can be copied infinitely.
CWE avoids this problem because:

* The *token* is what grants access
* The file itself is fully encrypted
* Users cannot decrypt content without owning the token

This creates a legally clean separation between:

* The binary data (encrypted, globally replicated)
* The ownership rights (token-based)

Thus, CWE enables digital resale **without violating reproduction rights**.

---

## 7. Does This Require DRM?

**Absolutely not.**

CWE’s Governance Charter forbids:

* DRM
* Device locking
* Secure enclaves
* Watermarking
* HDCP-like systems

Ownership is enforced **cryptographically** through rights tokens, not hardware control.

---

## 8. Summary

CWE introduces a digital ecosystem where buying a digital album actually feels like buying something tangible:

* **True ownership**
* **Optional resale**
* **Seamless inheritance**
* **Optional collectible NFTs**
* **No DRM or central authority**

This is how digital media *should* work.

---

## Appendix: Why Other Platforms Cannot Offer This

Because they rely on:

* Centralized accounts
* Proprietary licenses
* DRM enforcement
* Hardware-level controls

CWE avoids all of this using:

* Open cryptography
* Signed manifests
* Transferable rights tokens
* Decentralized governance

This gives creators new business models and gives users real digital property for the first time.

