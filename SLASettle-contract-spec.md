# SLASettle Contract Specification (v0.2)

This specification documents the architecture, storage layouts, public interfaces, lifecycle rules, and security model of SLASettle's Soroban smart contracts on the Stellar network.

Two smart contracts make up the core protocol:

- `contracts/watcher_registry`: Tracks eligible watchers, enforces one-vote-per-round submission freshness, and aggregates votes per endpoint commitment without interpreting quorum.
- `contracts/sla_vault`: Holds bonded funds per SLA, enforces SLA parameters against registry capacity, queries endpoint-bound round tallies, evaluates quorum, and settles breach penalties.

---

## v0.2 Architectural Changes & Breaking Upgrades

The v0.2 contract architecture introduces the following breaking interface and behavioral changes relative to v0.1:

| Component | v0.1 Design | v0.2 Design | Impact |
|---|---|---|---|
| **Endpoint Binding** | Unstored transaction parameter in `submit_check`; not bound to SLA or tallies | Authoritative `endpoint_hash: BytesN<32>` committed in `SLAConfig` and `create_sla`; tallies partitioned by `(sla_id, round_id, endpoint_hash)` | **Breaking**: Prevents cross-endpoint vote poisoning and enforces SLA commitment |
| **Watcher Registry Tallies** | `get_round_tally(sla_id, round_id)` | `get_round_tally(sla_id, round_id, endpoint_hash)` | **Breaking**: Tally queries must specify the exact endpoint commitment |
| **Observation Round Freshness** | Caller-supplied arbitrary `round_id`; no clock checks | Ledger-derived `round = env.ledger().timestamp() / 60`; rejects rounds outside `[current_round - 1, current_round]` with `Error::InvalidRound` | **Behavioral**: Watchers cannot vote on future or stale observation rounds |
| **SLA Creation Parameters** | `create_sla` lacked `endpoint_hash`; accepted quorum > watcher set | `create_sla` requires `endpoint_hash`, validates `uptime_target_bps in 1..=10_000`, and enforces `0 < quorum_threshold <= watcher_count` | **Breaking**: Prevents misconfigured or impossible-to-settle SLAs |
| **Cancellation Idempotency** | Calling `cancel_sla` on cancelled SLA succeeded repeatedly and emitted redundant events | Rejects second cancellation with `Error::SlaNotActive`; emits exactly one `SlaCancelled` event | **Behavioral**: Strict lifecycle transition |
| **Contract TTL Management** | Instance storage was never renewed on-chain; live contracts could expire | Auto-renews instance/code and persistent TTL on state changes; adds public `extend_contract_ttl` maintenance entrypoint | **New**: Proactive archival prevention |

---

## 1. `contracts/watcher_registry`

### 1.1 Storage Keys & TTL Policy

- `DataKey::Admin` (instance): Administrator address initialized once.
- `DataKey::Watcher(Address)` (persistent): Set membership indicating registered watcher status.
- `DataKey::WatcherCount` (instance): Monotonic count of registered watchers.
- `DataKey::Check(u64, u64, Address)` (persistent): Keyed by `(sla_id, round_id, watcher)`. Stores `CheckRecord { endpoint_hash, status }`.
- `DataKey::Tally(u64, u64, BytesN<32>)` (persistent): Keyed by `(sla_id, round_id, endpoint_hash)`. Stores `RoundTally { votes_up, votes_down }`.
- `DataKey::Paused` (instance): Emergency pause flag.

**TTL Extension Parameters:**
- `PERSISTENT_TTL_EXTEND_TO = 518_400` ledgers (~30 days at 5s/ledger).
- `PERSISTENT_TTL_THRESHOLD = 501_120` ledgers (re-extended within ~1 day of expiry).
- `INSTANCE_TTL_EXTEND_TO = 518_400` ledgers.
- `INSTANCE_TTL_THRESHOLD = 501_120` ledgers.

Every state write re-extends both the persistent entry and the contract instance/code TTL.

### 1.2 Data Types

```rust
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckStatus {
    Up,
    Down,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckRecord {
    pub endpoint_hash: BytesN<32>,
    pub status: CheckStatus,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub struct RoundTally {
    pub votes_up: u32,
    pub votes_down: u32,
}

#[contracterror]
#[repr(u32)]
pub enum Error {
    NotAuthorized = 1,
    AlreadyInitialized = 2,
    NotAWatcher = 3,
    DuplicateCheck = 4,
    ContractPaused = 5,
    InvalidRound = 6,
}
```

### 1.3 Public Interface

| Function | Auth | Description |
|---|---|---|
| `initialize(admin: Address)` | `admin` | One-time setup. Returns `Error::AlreadyInitialized` if called again. |
| `register_watcher(caller: Address, watcher: Address)` | `caller` (admin) | Idempotently registers `watcher` and increments `WatcherCount`. |
| `remove_watcher(caller: Address, watcher: Address)` | `caller` (admin) | Removes `watcher` and decrements `WatcherCount`. Safe no-op if unregistered. |
| `is_watcher(watcher: Address) -> bool` | None | Read whether `watcher` is registered. |
| `get_watcher_count() -> u32` | None | Returns total registered watchers. |
| `pause(caller: Address)` / `unpause(caller: Address)` | `caller` (admin) | Halts incoming `submit_check` transactions while allowing reads and admin management. |
| `get_current_round() -> u64` | None | Returns ledger close time divided by `ROUND_LENGTH_SECONDS` (60s). |
| `extend_contract_ttl()` | None | Extends contract instance and code TTL to 518,400 ledgers without altering business state. |
| `has_watcher_voted(sla_id: u64, round_id: u64, watcher: Address) -> bool` | None | Returns `true` if `watcher` has submitted a check for `(sla_id, round_id)`. |
| `submit_check(watcher: Address, sla_id: u64, round_id: u64, endpoint_hash: BytesN<32>, status: CheckStatus)` | `watcher` | Validates watcher registration, round freshness, and single-vote constraint. Records vote and increments endpoint-partitioned tally. |
| `get_round_tally(sla_id: u64, round_id: u64, endpoint_hash: BytesN<32>) -> RoundTally` | None | Reads vote tallies for `(sla_id, round_id, endpoint_hash)`. Returns `{ votes_up: 0, votes_down: 0 }` if unvoted. |

### 1.4 Observation Round & One-Vote Invariants

1. **Ledger-Clock Round Derivation:**
   - Observation round length is fixed: `ROUND_LENGTH_SECONDS = 60`.
   - Current round: `curr_round = env.ledger().timestamp() / 60`.
   - `submit_check` accepts strictly:
     - `round_id == curr_round` (active observation window), OR
     - `curr_round > 0 && round_id == curr_round - 1` (grace window for immediately preceding round).
   - Any future round (`round_id > curr_round`) or older round (`round_id < curr_round - 1`) is rejected with `Error::InvalidRound`.
2. **One-Vote-Per-Watcher Invariant:**
   - Uniqueness key is strictly `DataKey::Check(sla_id, round_id, watcher)`.
   - A watcher cannot cast multiple votes in a single round by varying `endpoint_hash`.
   - Submitting an unexpected `endpoint_hash` consumes that watcher's single round submission, increments only that mismatched hash's isolated tally, and cannot be resubmitted.
3. **Endpoint Tally Isolation:**
   - Tallies are keyed strictly by `DataKey::Tally(sla_id, round_id, endpoint_hash)`.
   - Votes submitted for hash A have zero effect on the tallies for hash B.

### 1.5 Events

| Event | Topics | Data |
|---|---|---|
| `WatcherRegistered` | `watcher: Address` | None |
| `WatcherRemoved` | `watcher: Address` | None |
| `CheckSubmitted` | `sla_id: u64`, `watcher: Address` | `round_id: u64`, `endpoint_hash: BytesN<32>`, `status: CheckStatus` |

---

## 2. `contracts/sla_vault`

### 2.1 Storage Keys & TTL Policy

- `DataKey::Admin` (instance): Administrator address.
- `DataKey::WatcherRegistry` (instance): Target `watcher_registry` contract address.
- `DataKey::NextSlaId` (instance): Incremental SLA ID counter.
- `DataKey::Sla(u64)` (persistent): Stores `SLAConfig`.
- `DataKey::BondBalance(u64)` (persistent): Stores `i128` remaining token bond balance.
- `DataKey::SettledRounds(u64, u64)` (persistent): Keyed by `(sla_id, round_id)`. Value `bool` indicating completed settlement payout.
- `DataKey::Paused` (instance): Gates new SLA creation.

All persistent writes and instance mutations execute TTL renewal with `518_400` ledgers extension.

### 2.2 Data Types

```rust
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SLAStatus {
    Active,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SLAConfig {
    pub provider: Address,
    pub token: Address,
    pub endpoint_hash: BytesN<32>,
    pub bond_amount: i128,
    pub uptime_target_bps: u32,
    pub quorum_threshold: u32,
    pub penalty_per_breach: i128,
    pub beneficiary: Address,
    pub status: SLAStatus,
}

#[contracterror]
#[repr(u32)]
pub enum Error {
    NotAuthorized = 1,
    AlreadyInitialized = 2,
    SlaNotActive = 3,
    AlreadySettled = 4,
    QuorumNotMet = 5,
    BondExhausted = 6,
    InvalidAmount = 7,
    ContractPaused = 8,
    SlaNotFound = 9,
    InvalidUptimeTarget = 10,
    InvalidQuorum = 11,
}
```

### 2.3 Public Interface

| Function | Auth | Description |
|---|---|---|
| `initialize(admin: Address, watcher_registry: Address)` | `admin` | Sets admin and registry contract bindings. |
| `create_sla(provider: Address, token: Address, endpoint_hash: BytesN<32>, bond_amount: i128, uptime_target_bps: u32, quorum_threshold: u32, penalty_per_breach: i128, beneficiary: Address) -> Result<u64, Error>` | `provider` | Locks `bond_amount` tokens, validates parameters, and stores `SLAConfig`. Returns `sla_id`. |
| `get_sla(sla_id: u64) -> Result<SLAConfig, Error>` | None | Returns stored SLA configuration. |
| `get_bond_balance(sla_id: u64) -> i128` | None | Returns current remaining bond balance. |
| `top_up_bond(caller: Address, sla_id: u64, amount: i128)` | `caller` (provider) | Deposits additional tokens to replenish bond balance. |
| `trigger_settlement(caller: Address, sla_id: u64, round_id: u64)` | None (permissionless) | Queries registry tally for `config.endpoint_hash`. If `votes_down >= quorum_threshold`, transfers `min(penalty, balance)` to beneficiary. |
| `is_round_settled(sla_id: u64, round_id: u64) -> bool` | None | Returns whether round has settled payout. |
| `cancel_sla(caller: Address, sla_id: u64)` | `caller` (provider) | Transitions active SLA to `Cancelled`. Rejects if already cancelled with `Error::SlaNotActive`. |
| `withdraw_remaining_bond(caller: Address, sla_id: u64)` | `caller` (provider) | Returns unbreached bond balance to provider once SLA is `Cancelled`. |
| `pause(caller: Address)` / `unpause(caller: Address)` | `caller` (admin) | Halts new SLA creation (`create_sla`) while preserving settlements, cancellations, top-ups, and withdrawals. |
| `extend_contract_ttl()` | None | Extends contract instance and code TTL to 518,400 ledgers. |

### 2.4 Settlement & Lifecycle Rules

1. **Endpoint Commitment Binding:**
   - `trigger_settlement` loads `config.endpoint_hash` from persistent contract storage.
   - Caller cannot supply an endpoint hash. The SLA's committed hash is authoritative.
   - Settlement queries `registry.get_round_tally(sla_id, round_id, config.endpoint_hash)`.
   - Votes cast for another endpoint hash cannot trigger breach settlement.
2. **Historical Valid-Round Settlement:**
   - Round freshness checks apply strictly to `submit_check`, **not** to `trigger_settlement`.
   - Any historical round that successfully gathered quorum while fresh remains eligible for settlement later, provided the SLA is active and the round was not already settled.
3. **Quorum & Parameter Bounds:**
   - `bond_amount > 0`
   - `0 < penalty_per_breach <= bond_amount`
   - `1 <= uptime_target_bps <= 10_000` (basis points; validated descriptive parameter)
   - `0 < quorum_threshold <= watcher_registry.get_watcher_count()` (cannot exceed registry capacity)
4. **Conservation of Funds:**
   - Payout per breach is `min(penalty_per_breach, remaining_balance)`.
   - Bond balance never drops below zero.
   - Payouts stop cleanly with `Error::BondExhausted` when balance is 0.

### 2.5 Events

| Event | Topics | Data |
|---|---|---|
| `SlaCreated` | `sla_id: u64`, `provider: Address` | `token: Address`, `endpoint_hash: BytesN<32>`, `bond_amount: i128`, `uptime_target_bps: u32`, `quorum_threshold: u32`, `penalty_per_breach: i128`, `beneficiary: Address` |
| `BondToppedUp` | `sla_id: u64` | `amount: i128` |
| `SettlementPaid` | `sla_id: u64`, `round_id: u64` | `payout: i128`, `beneficiary: Address` |
| `SlaCancelled` | `sla_id: u64` | None |
| `BondWithdrawn` | `sla_id: u64` | `amount: i128` |

---

## 3. Off-Chain Commitment Preimage Specification

Contracts receive `BytesN<32>` commitments and never parse URLs or calculate SHA-256 hashes on-chain.

The canonical off-chain commitment preimage is defined as:
```
endpoint_hash = SHA-256("slasettle:endpoint:v1:" + trimmedEndpoint)
```
where `trimmedEndpoint` is the input URL string with only leading and
trailing ASCII whitespace (space, `\t`, `\n`, `\r`, `\f`, `\v`) removed.

**Preimage Rules:**
- The ASCII trim is the only normalization applied. Scheme, host, port, path,
  query string, and casing must otherwise match exactly byte-for-byte across
  SDK, provider registration, watcher HTTP checks, and indexer verification —
  no lowercasing, no URL canonicalization, no percent-decoding, no trailing-
  slash normalization, inside contracts or off-chain tooling.
- `"slasettle:endpoint:v1:"` is a fixed domain-separation prefix, concatenated
  with `trimmedEndpoint` and UTF-8 encoded before hashing. It exists so this
  preimage can never collide with a hash computed for an unrelated purpose
  that happens to hash the same raw bytes.
- Cross-language utilities in Go (`crypto/sha256`) and TypeScript (Node
  `crypto.createHash('sha256')`) must evaluate the identical digest for the
  same preimage.

---

## 4. Disclosed Protocol Limitations

Reviewers, operators, and integrators must be aware of the following residual design boundaries:

1. **Endpoint Commitment Does Not Prove Truthful Observation:**
   - Committing to `endpoint_hash` proves that watchers and providers agreed on the target URL.
   - It does **not** cryptographically prove that a watcher executed a real HTTP GET check or reported truthful results.
   - Fault tolerance and security depend on independent watcher operation and quorum assumptions.
2. **Absence of Commit-Reveal:**
   - Votes in `watcher_registry` are visible immediately on-chain and in the public transaction mempool.
   - A malicious or lazy watcher could observe peer submissions before submitting their own vote for the round.
   - Commit-reveal is deferred to future protocol iterations to maintain lower transaction costs and sub-minute settlement responsiveness.
3. **Shared Global Watcher Set:**
   - Watchers registered in `watcher_registry` are authorized to vote on any active SLA.
   - Per-SLA watcher sub-committees are not implemented in v0.2.
4. **Descriptive `uptime_target_bps`:**
   - While strictly validated (`1..=10_000`), `uptime_target_bps` serves as descriptive service level metadata.
   - Settlement remains discrete per-round penalty payouts rather than cumulative monthly uptime percentage calculations.
5. **Testnet-Only Scope:**
   - Contracts are deployed and evaluated on Stellar Testnet (Protocol 29 at v0.2 deployment/cutover verification; Testnet may subsequently upgrade).
   - Parameters and rent TTL values have not been calibrated for mainnet production economic stakes.
