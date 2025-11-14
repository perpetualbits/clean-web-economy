<!-- File: docs/faq/storage_and_erasure_coding.md -->

# FAQ: Storage Requirements & Erasure Coding Feasibility

**Version:** Draft v1.0
**Status:** Public-Facing FAQ Article

---

## 1. Short Answer

Yes — erasure coding is not only feasible but ideal for the Clean Web Economy (CWE). Even if CWE stored **10% of the world’s music and film output**, a global volunteer network of **25,000–50,000 nodes** would comfortably provide redundant, durable storage.

Erasure coding dramatically reduces redundancy cost while increasing resilience to node churn.

---

## 2. How Much Storage Does CWE Actually Need?

Let’s estimate “world-scale media” in realistic terms.

### **2.1 Approximate Sizes**

* Global audiovisual catalog (music + films + series): **25–30 PB**
* CWE stores only a fraction: long tail + independent + select mainstream

Assume CWE stores **10%** → ≈ **2.5 PB** of content.

### **2.2 With 5× redundancy** (typical decentralized target)

```
2.5 PB × 5 = 12.5 PB total required storage
```

---

## 3. How Many Nodes Would Be Needed?

If an average volunteer allocates **500 GB** (0.5 TB), which fits comfortably on:

* A Raspberry Pi with USB SSD
* A cheap mini-PC
* An old desktop with a spare drive

Then required nodes:

```
12.5 PB / 0.5 TB = 25,000 nodes
```

This is *trivial* for a global volunteer community.
Even early-stage decentralized networks exceed this scale.

---

## 4. Why Erasure Coding Is Better Than Replication

### **Replication (simple 1:5 mirroring) drawbacks:**

* Large overhead
* Poor resilience to correlated failures
* Wasteful for long-tail content

### **Erasure coding advantages:**

* Recover any chunk from **k of (k+m)** pieces
* Withstand massive churn
* Reduce bandwidth for repairs
* Optimize for cold storage

Example configuration:

```
k = 20 data shards
m = 10 parity shards
```

This means:

* Only 20 of 30 shards required for reconstruction
* Can lose **up to 33%** of nodes simultaneously with zero data loss

---

## 5. How Repair Works

CWE uses DMF Storage Coordination Nodes to:

* Track missing shards
* Detect offline nodes
* Reconstruct lost shards using parity
* Redistribute shards to restore redundancy

This process is automatic and **does not require user identity** or node-level surveillance.

---

## 6. What About Node Churn?

Nodes go offline all the time:

* Power loss
* ISP outages
* Reboots
* Users retiring old hardware

Erasure coding is *designed* for this environment.

With k=20, m=10:

* Up to **10 nodes** in a shard group may disappear indefinitely with no risk
* Repairs happen only when losses exceed thresholds

This is far superior to traditional P2P systems.

---

## 7. Does Encrypted Storage Make This Harder?

No.
CWE stores **only encrypted blobs**, without metadata.
Nodes do not know:

* Which chunks belong to which file
* Which creator owns the content
* Whether content is audio, video, or nonsense

This actually **reduces legal risk** and **improves reliability**, because nodes cannot selectively censor or drop content.

---

## 8. Can We Support HD and 4K Video?

Yes.

Storage and bandwidth are not limiting factors because:

* Only shards, not full files, are moved
* Erasure coding reduces replication overhead
* Popular content is cached more heavily

CWE expects:

* Music: modest size (5–100 MB)
* HD films: 4–12 GB
* 4K: 10–40 GB

Even if 4K becomes dominant, volunteer networks can scale linearly.

---

## 9. What About Permanent Archival?

CWE inherently supports:

* Decentralized long-term storage
* No single point of failure
* Global redundancy

Creators may optionally pay for “archival mode,” where nodes receive microrewards for holding rarely-accessed but culturally important content.

---

## 10. What Happens if Only 5,000 Nodes Exist?

The network would still work.

Storage would simply:

* Reduce redundancy factors
* Increase rebuild frequency
* Possibly reduce long-tail availability temporarily

But the system is self-healing and self-scaling.

---

## 11. Why Critics Are Wrong to Assume CWE Cannot Store Enough Data

Common misconceptions:

1. “You’d need millions of nodes!” → **False. Tens of thousands is enough.**
2. “Video is too heavy for volunteers.” → **False. Erasure coding + caching solves this.**
3. “Node churn would destroy files.” → **False. Parity groups handle massive churn.**
4. “Legal issues make storage risky.” → **False. Chunks are encrypted and unidentifiable.**

CWE resembles IPFS/Bittorrent-like resilience with added compliance and reliability.

---

## 12. Summary

CWE can store vast amounts of media with a global volunteer network because:

* Erasure coding reduces storage overhead
* Encrypted chunks remove legal and privacy risks
* Automatic repair ensures durability
* Even 25k–50k nodes globally provide industrial-grade capacity

This is not only feasible — **it is future-proof and scalable**.

CWE stands to become one of the most robust, equitable, and censorship-resistant media storage systems ever deployed.

