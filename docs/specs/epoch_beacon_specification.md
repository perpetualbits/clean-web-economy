<!-- File: docs/specs/epoch_beacon_specification.md -->

# Clean Web Economy

## Epoch Beacon Specification

**Version:** Draft v1.0
**Status:** Design Document (For DAO Review)

---

## 1. Purpose

This specification defines the **Epoch Beacon** — a cryptographically secure, globally verifiable, and privacy-preserving randomness signal that anchors:

* Pseudonymization of user usage events
* Tier Capability Token rotation
* Bandwidth receipt validity windows
* Rollup boundaries for DAPR aggregation
* Temporal structure for ZK proofs

The Epoch Beacon provides a **public, unbiased, unpredictable value per epoch**, enabling unlinkability and preventing replay attacks without revealing anything about users.

---

## 2. Requirements

The Epoch Beacon MUST:

### 2.1 Security

* Be unpredictable until published
* Be publicly verifiable
* Resist manipulation by any single actor
* Be stable and immutable once published
* Be safe against chain reorganizations (after finality)

### 2.2 Privacy

* Provide unlinkability across epochs
* Prevent correlation of commitments over time
* Not depend on user data
* Not reveal consumption behavior

### 2.3 Availability

* Be accessible via:

  * Chain contract calls
  * CDN mirrors
  * Discovery hubs
  * Public hosted endpoints

### 2.4 Interoperability

* Must work with:

  * ZK Usage Proof circuits
  * Tier Capability Token Format
  * Bandwidth Receipt Protocols
  * Client–Storage Handshake

---

## 3. Epoch Structure

### 3.1 Epoch Length

Governance-defined, typical value:

```
24 hours (UTC)
```

Shorter or longer epochs MAY be adopted by governance.

### 3.2 Epoch Index

Each epoch has an index:

```
E = floor( timestamp / epoch_duration )
```

### 3.3 Epoch Beacon Value

For each epoch `E`, the beacon is:

```
K_epoch = H( R_chain_finalized_block || E )
```

Where:

* `R_chain_finalized_block` is the randomness available from a finalized block in the rollup or L1 chain.
* `H()` is a cryptographic hash (e.g., Poseidon, SHA-256, Keccak).

---

## 4. Generation Model

The Epoch Beacon MAY be derived from:

### 4.1 On-Chain Source (Preferred)

Using a finalized block randomness field, such as:

* Ethereum Beacon chain RANDAO
* L2 sequencer-provided randomness (if unbiased)
* VRF-based randomness from on-chain oracles

### 4.2 Multi-Party Computation (Optional Backup)

If chain randomness is unavailable or insufficient:

* DMF nodes perform MPC to generate `R_mpc`
* Publish commitments then final reveal
* Combine with chain randomness when available

### 4.3 Hybrid Model

Default recommendation:

```
R_final = H( R_chain || R_mpc )
```

This ensures:

* Manipulation requires breaking *both* the chain and the MPC committee
* Bias resistance

### 4.4 Final Epoch Beacon

```
K_epoch = H( R_final || E )
```

---

## 5. Client Responsibilities

Clients MUST:

### 5.1 Fetch Beacon

Retrieve `K_epoch` from:

* Chain contract
* Trusted mirrors
* Discovery hubs
* Caching layers

### 5.2 Validate Beacon

Clients MUST check:

* Beacon index matches expected epoch
* Hash commitment matches chain-published value
* No client-local override exists

### 5.3 Use Beacon for Privacy

The beacon is used to derive:

* Pseudonyms for usage commitments:

  ```
  P_w,j = H( K_epoch || C_w,j )
  ```
* Tier capability rotation keys:

  ```
  T_epoch = H( K_epoch || T_commit )
  ```
* Bandwidth receipt epochs:

  ```
  receipt_epoch = E
  ```

### 5.4 Discard After Use

After epoch rollover:

* Clients MUST discard old beacon values
* All pseudonyms MUST be re-derived
* All capability tokens MUST rotate

---

## 6. Microservice Responsibilities

### 6.1 Access Microservice

MUST:

* Accept only capability tokens consistent with `K_epoch`
* Reject tokens from previous epochs

### 6.2 Aggregators (DMF)

MUST:

* Accept ZK proofs bound to the correct epoch beacon
* Use `K_epoch` in MPC attribution pipelines

### 6.3 Discovery Hubs

MUST NOT:

* Log beacon fetch requests in a linkable manner
* Correlate IP addresses with access frequency

---

## 7. Security Considerations

### 7.1 Replay Protection

Because `K_epoch` changes every epoch, attackers cannot:

* Replay old pseudonyms
* Reuse old capability tokens
* Forge bandwidth receipts

### 7.2 Beacon Manipulation Resistance

Using hybrid randomness ensures:

* No single actor controls randomness
* Sequencer cannot bias values
* MPC reveals are publicly logged

### 7.3 Robustness Against Network Splits

During temporary partitions:

* Clients MAY continue using the last known `K_epoch`
* Once reconnected, clients MUST switch to the correct beacon

### 7.4 Zero-Knowledge Stability

`K_epoch` provides stable inputs for circuits, ensuring:

* Predictable proving cost
* Simple verification logic
* No sensitive data dependence

---

## 8. Integration With Other Specifications

### 8.1 ZK Usage Proofs

The beacon ensures each epoch’s pseudonyms are unlinkable:

```
P_w,j = H( K_epoch || C_w,j )
```

### 8.2 Tier Capability Tokens

The beacon creates epoch-unique capability tokens:

```
T_epoch = H( K_epoch || T_commit )
```

### 8.3 Client–Storage Handshake

Receipts include epoch binding:

```
R_bound = Com( payload , K_epoch )
```

### 8.4 DAPR Weighting

Epoch boundaries define aggregation boundaries:

* Only events with matching `K_epoch` accepted

---

## 9. Failure Modes & Mitigations

### 9.1 Missing Beacon

If client cannot fetch beacon:

* MAY continue using cached beacon
* MUST revalidate when online
* MUST NOT skip epoch transitions

### 9.2 Forked Beacon

If chain fork creates multiple possible beacons:

* Light clients verify via finality
* DMF publishes canonical beacon

### 9.3 Malicious Mirrors

Client MUST verify beacon hash against on-chain value.

---

## 10. Summary

The Epoch Beacon is the temporal and cryptographic anchor for CWE’s privacy and integrity model. It:

* Enables unlinkability across epochs
* Prevents replay attacks
* Supports ZK circuits and aggregation
* Requires no trusted hardware or central authority
* Ensures the entire ecosystem maintains privacy while remaining tamper-resistant

The Epoch Beacon thus plays a foundational role in the secure, decentralized operation of the Clean Web Economy.

