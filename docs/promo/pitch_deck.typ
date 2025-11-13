// Clean Web Economy — Pitch Deck (Typst)
// Usage: typst compile docs/promo/pitch_deck.typ pdf/promo/pitch_deck.pdf

// Page & typography
#set page(width: 16in, height: 9in, margin: 1in)
#set text(font: "Inter", size: 36pt) // Inter must be installed; else remove font: or use "Liberation Sans"
#set align(center)

// Slide helper (no pagebreak inside)
#let slide(title, body) = block[
  #align(center)[
    #text(size: 72pt, weight: "bold")[#title]
    #v(24pt)
    #body
  ]
]

// Cover
#slide("Clean Web Economy", [
  #text(size: 32pt)[Let’s fund culture without selling our souls.]
])

#pagebreak()

// Problem
#slide("The Problem", [
  • Ads distort the web and privacy.\
  • Creators are underpaid.\
  • Users pay multiple subscriptions.
])

#pagebreak()

// Insight
#slide("The Insight", [
  Flat-fee tiers + privacy-preserving usage proofs → direct creator payouts.
])

#pagebreak()

// How it Works
#slide("How It Works", [
  1. Recognize work (fingerprint).\
  2. Track locally.\
  3. Submit zero-knowledge proof.\
  4. Smart contracts split fees.
])

#pagebreak()

// Stack
#slide("The Stack", [
  Clients • Chain • Storage • DMF • DAO
])

#pagebreak()

// Resilience
#slide("Resilience by Design", [
  SSI + ZK + Audits + Councils + SBOM + Anycast.
])

#pagebreak()

// Adoption
#slide("Adoption Plan", [
  Phase 1 Music → Phase 2 Video → Phase 3 DMF → Phase 4 Governance.
])

#pagebreak()

// CTA
#slide("Call to Action", [
  Join as engineer, creator, lawyer, auditor, activist.
])

#pagebreak()

// Vision
#slide("Vision", [
])

