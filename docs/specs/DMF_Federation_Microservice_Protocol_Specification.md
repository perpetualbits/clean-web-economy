<!-- File: docs/specs/dmf_federation_microservice_protocol_specification.md -->

# Clean Web Economy

## DMF Federation & Microservice Protocol Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

The **Distributed Microservices Fabric (DMF)** is the decentralized backbone of the Clean Web Economy (CWE). It provides:

* Federation of independent operators
* Reliable content distribution coordination
* Stateless, privacy-respecting microservice execution
* Redundancy and fault-tolerance for metadata, manifests, and storage coordination
* A hardened, censorship-resistant service substrate

This specification defines:

* The DMF federation model
* Microservice manifest structure
* Service discovery & routing
* Protocol-level security requirements
* Compliance rules
* Anti-abuse and anti-centralization measures

DMF is *not* a blockchain. It is a mesh of independent HTTPS/TLS services operating under shared cryptographic rules.

---

## 2. Design Principles

### 2.1 Federation, Not Centralization

No DMF node holds privileged authority.
Any university, cooperative, NGO, or creator group may operate nodes.

### 2.2 Privacy First

DMF nodes MUST NOT:

* Track users
* Correlate requests
* Collect IP-level identifiers
* Use cookies or sessions

### 2.3 Deterministic, Cryptographic Behavior

DMF nodes must:

* Validate manifests and signatures
* Publish signed microservice manifests
* Perform integrity checks deterministically

### 2.4 Stateless Execution

Services MUST be stateless or pseudo-stateless, except for:

* Storage metadata
* Content discovery indices

### 2.5 Auditability

All DMF outputs must be:

* Publicly verifiable
* Independently reproducible
* Compatible with ZK proofs where applicable

---

## 3. DMF Node Types

### 3.1 Metadata Nodes

Responsibilities:

* Index creator manifests
* Validate signatures
* Publish categorization snapshots

### 3.2 Storage Coordination Nodes

Responsibilities:

* Track chunk replication
* Assign redundancy groups
* Orchestrate erasure coding
* Trigger repairs

### 3.3 Discovery Nodes

Responsibilities:

* Host the decentralized search index
* Enforce ranking logic (per spec)
* Mirror global metadata

### 3.4 Aggregation Coordinators

Responsibilities:

* Receive ingress usage commitments
* Coordinate batching to aggregation nodes
* Forward rollup proofs to settlement pipeline

### 3.5 Optional Specialty Nodes

Examples:

* Language model metadata classifiers
* Fingerprint similarity evaluators
* AI moderation or categorization tools

---

## 4. Federation Model

The DMF uses a **signed microservice manifest** that declares the capabilities of each node.

### 4.1 Node Manifest Structure

```
{
  "node_id": DID,
  "operator": optional DID,
  "services": ["metadata", "discovery", ...],
  "capabilities": {...},
  "public_key": ...,   // for service signing
  "mirrors": [...],
  "version": semver,
  "signature": creator/operator signature
}
```

### 4.2 Node Admission

Nodes join DMF by:

* Publishing a signed manifest
* Passing integrity checks
* Passing minimal uptime & availability checks
* Being indexed by at least 2 independent nodes

### 4.3 Voluntary Federation

No central authority approves nodes.
Nodes organically federate by:

* Mutual validation
* Cross-indexing
* Shared cryptographic standards

---

## 5. DMF Microservice Specifications

### 5.1 Service Manifest

Each microservice publishes:

```
{
  "service_id": UUID,
  "type": "storage_coord" | "metadata_index" | ...,
  "entrypoints": [URL...],
  "schema": OpenAPI or protobuf,
  "version": semver,
  "signature": node signature
}
```

All clients MUST verify service manifests before interacting.

### 5.2 Stateless Protocol

Requests MUST be:

* Stateless
* Idempotent
* Without cookies, sessions, or per-user tokens

DMF nodes cannot track user identity.

### 5.3 Required Endpoints (High-Level)

#### 5.3.1 Metadata Nodes

```
GET /manifest/<id>
GET /creator/<id>
GET /category/<tag>
POST /publish_manifest
```

#### 5.3.2 Storage Coordination

```
GET /chunk_status/<cid>
POST /repair_task
POST /assign_erasure_group
```

#### 5.3.3 Discovery Nodes

```
GET /search
GET /trending
GET /recommendations (aggregate-only)
```

#### 5.3.4 Aggregation Coordinator

```
POST /ingress
GET /batch_status/<epoch>
POST /submit_rollup
```

---

## 6. Security Requirements

### 6.1 TLS Everywhere

All DMF communication MUST use:

* TLS 1.3
* Forward secrecy
* Modern cipher suites

### 6.2 Request Hardening

* No cookies
* No persistent headers
* Rate limits based only on IP/subnet heuristic, not identification

### 6.3 Integrity of Metadata

DMF nodes MUST validate:

* Creator signatures
* Manifest structure
* Fingerprints

### 6.4 Anti-Poisoning Controls

Nodes must reject:

* Malformed manifests
* Incorrect fingerprints
* Duplicate-upload attacks
* Metadata spam

### 6.5 Adversarial Node Controls

A malicious DMF node CANNOT:

* Modify manifests (signature mismatch)
* Influence rollups (ZK validity enforced)
* Correlate user events (no stateful API)

---

## 7. Storage Federation

DMF coordinates redundancy across nodes.

### 7.1 Erasure Coding Support

Nodes MAY participate in:

* Reed–Solomon groups
* LRC (Local Reconstruction Codes)
* XOR parity meshes

### 7.2 Chunk Placement Algorithm

DMF uses:

* Region diversity
* Node capacity
* Node uptime
* Chunk popularity tier

Goals:

* Robust against node churn
* Efficient hot content distribution
* Efficient long-tail preservation

### 7.3 Repair Protocol

Nodes issue repair tasks:

```
POST /repair_task
```

Containing:

* chunk ID
* parity group info
* instructions for reconstruction

---

## 8. Aggregation Coordination

DMF plays a critical role in usage aggregation:

* Ingress nodes collect commitments
* DMF distributes validation load
* DMF merges partial batches
* DMF assigns a rollup constructor

Ensures:

* High throughput
* Fault tolerance
* Redundant validation paths

---

## 9. Anti-Abuse Mechanisms

### 9.1 Sybil Node Detection

Nodes may be flagged for:

* Excessive manifest churn
* Bad fingerprint submissions
* Serving invalid metadata

### 9.2 Node Reputation (Optional)

Nodes may maintain *operator reputation* based on:

* Uptime
* Error rate
* Metadata correctness

Reputation MUST NOT:

* Track user behavior
* Control client access

### 9.3 Quarantine & Delisting

Nodes may be:

* Quarantined from federation
* Delisted by peers
* Reported to DAO for permanent removal

---

## 10. Privacy Guarantees

DMF nodes MUST NOT:

* Track users
* Create per-user logs
* Insert tracking headers
* Export logs to centralized systems

DMF is structured to be **content-coordinating**, not user-observing.

---

## 11. Governance Integration

DAO governs:

* DMF specs
* Node compliance requirements
* Federation protocol upgrades
* Security guidelines

Upgrade process must be:

* Versioned
* Backward-compatible where possible
* Open for public review

---

## 12. Summary

The DMF Federation & Microservice Protocol creates:

* A decentralized, censorship-resistant infrastructure
* A privacy-preserving service layer
* A cryptographically verifiable coordination mesh
* A robust system for discovery, storage, repair, and aggregation

DMF is the backbone that keeps CWE open, reliable, and secure—without central servers, without surveillance, and without single points of failure.

