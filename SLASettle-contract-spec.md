# SLASettle contract specification

This document did not previously exist in this repository, even though three
places in the source (`README.md`, `contracts/watcher_registry/src/lib.rs`,
`contracts/sla_vault/src/storage.rs`) referenced it by name. Git history in
both `slasettle-vault` and `slasettle-hub` was checked and shows no prior
version of this file. It is reconstructed here directly from the current
implemented contract source, not from memory or invented design intent.
Where a claim below cannot be traced to a specific function or storage
definition in the current `main` branch, it is not included.

Two contracts:

- `contracts/watcher_registry`: tracks an eligible set of watchers and counts
  independent up/down votes for a given `(sla_id, round_id)`. It has no
  concept of a quorum threshold and is not meant to.
- `contracts/sla_vault`: holds a provider's bonded funds per SLA, reads the
  raw vote tally from `watcher_registry`, applies a quorum judgment, and pays
  a fixed penalty to the beneficiary when quorum is reached.

## `watcher_registry`

### Storage (`src/storage.rs`)

- `DataKey::Admin` (instance): the admin address set at `initialize`.
- `DataKey::Watcher(Address)` (persistent): presence marks an address as an
  eligible watcher.
- `DataKey::WatcherCount` (instance): running count of registered watchers.
- `DataKey::Check(sla_id, round_id, watcher)` (persistent): one watcher's
  vote for one round, keyed to prevent duplicate submission.
- `DataKey::Tally(sla_id, round_id)` (persistent): running `RoundTally { votes_up, votes_down }`.
- `DataKey::Paused` (instance): pause flag.
- Persistent entries are TTL-extended to roughly 30 days
  (`PERSISTENT_TTL_EXTEND_TO = 518_400` ledgers) whenever written, re-extended
  once within about a day of expiry.

### Types

- `CheckStatus`: `Up` or `Down`.
- `Error`: `NotAuthorized = 1`, `AlreadyInitialized = 2`, `NotAWatcher = 3`,
  `DuplicateCheck = 4`, `ContractPaused = 5`.

### Functions and authorization

| Function | Auth | Notes |
|---|---|---|
| `initialize(admin)` | `admin` | One-time; fails with `AlreadyInitialized` if called again. |
| `register_watcher(caller, watcher)` | `caller` must be admin | Idempotent: registering an already-registered watcher does not double the count. |
| `remove_watcher(caller, watcher)` | `caller` must be admin | Removing a watcher that was never registered is a no-op, not an error. |
| `is_watcher(watcher)` | none | Read. |
| `get_watcher_count()` | none | Read. |
| `pause(caller)` / `unpause(caller)` | `caller` must be admin | Blocks `submit_check` while paused; does not block reads. |
| `submit_check(watcher, sla_id, round_id, endpoint_hash, status)` | `watcher` | `watcher` must already be registered. Exactly one vote per `(sla_id, round_id, watcher)`; a duplicate is rejected with `DuplicateCheck`, never overwritten. `endpoint_hash` is accepted but **not stored and not emitted**: the check record holds only the `CheckStatus`, and `CheckSubmitted` carries no hash (`watcher_registry/src/lib.rs` discards it with `let _ = &endpoint_hash`). It is visible only as an argument of the submitting transaction, where a human or another system could read it from transaction history; this contract neither validates nor keeps it. (The doc comment above `submit_check` still says "stored on the check record"; that comment is stale.) |
| `get_round_tally(sla_id, round_id)` | none | Read. |
| `has_watcher_voted(sla_id, round_id, watcher)` | none | Read. |

### Events (`src/events.rs`)

| Event | Topics | Data |
|---|---|---|
| `WatcherRegistered` | `watcher: Address` | (none) |
| `WatcherRemoved` | `watcher: Address` | (none) |
| `CheckSubmitted` | `sla_id: u64`, `watcher: Address` | `round_id: u64`, `status: CheckStatus` |

On the wire, topic 0 of every event is the event's name as a snake_case
symbol (`watcher_registered`, `watcher_removed`, `check_submitted`), and the
`Topics` column above lists only the topics that follow it. Data fields are
carried as a map keyed by field name; an event with no data fields carries an
empty map. `CheckStatus` is encoded as a one-element vec holding the variant's
symbol (`["Up"]`, `["Down"]`). All three event kinds have been observed on
real Testnet transactions and decoded; see
`evidence/testnet-2026-09-27.md` (all 8 event kinds table).

### Known limitation: no commit-reveal

Because contract state is public, a watcher who submits late can see how
earlier watchers voted before deciding what to submit, and could copy the
emerging majority instead of reporting what they actually observed. The
standard fix is a commit-reveal scheme, which roughly doubles the
transaction count per watcher per round. This version ships without it.
Revisit before this handles anything beyond a demo bond.

## `sla_vault`

### Storage (`src/storage.rs`)

- `DataKey::Admin` (instance): admin address set at `initialize`.
- `DataKey::WatcherRegistry` (instance): the registry contract address this
  vault queries.
- `DataKey::NextSlaId` (instance): monotonically increasing SLA id counter.
- `DataKey::Sla(u64)` (persistent): the `SLAConfig` for that SLA id.
- `DataKey::BondBalance(u64)` (persistent): remaining bond for that SLA id.
- `DataKey::SettledRounds(sla_id, round_id)` (persistent): presence marks that
  round as already settled, enforcing settlement idempotency.
- `DataKey::Paused` (instance): pause flag, gates `create_sla` only (see
  below).
- Same persistent TTL policy as `watcher_registry`: extended to roughly 30
  days on write.

### `SLAConfig`

```
provider: Address
token: Address
bond_amount: i128
uptime_target_bps: u32
quorum_threshold: u32
penalty_per_breach: i128
beneficiary: Address
status: SLAStatus (Active | Cancelled)
```

`uptime_target_bps` is stored and can be displayed, but nothing in this
contract computes an actual uptime percentage over a billing period or
enforces that number. What actually happens is simpler: every time a round is
independently confirmed down by quorum, a fixed penalty fires. Aggregate
uptime enforcement is out of scope for this version.

### `Error`

`NotAuthorized = 1`, `AlreadyInitialized = 2`, `SlaNotActive = 3`,
`AlreadySettled = 4`, `QuorumNotMet = 5`, `BondExhausted = 6`,
`InvalidAmount = 7`, `ContractPaused = 8`, `SlaNotFound = 9`.

### Functions and authorization

| Function | Auth | Notes |
|---|---|---|
| `initialize(admin, watcher_registry)` | `admin` | One-time. |
| `create_sla(provider, token, bond_amount, uptime_target_bps, quorum_threshold, penalty_per_breach, beneficiary)` | `provider` | Rejects `bond_amount <= 0`, `penalty_per_breach <= 0`, `penalty_per_breach > bond_amount`, and `quorum_threshold == 0` (see below), all as `InvalidAmount`. Transfers `bond_amount` of `token` from `provider` into the vault in the same call. Returns the new `sla_id`. Blocked while paused. |
| `get_sla(sla_id)` / `get_bond_balance(sla_id)` | none | Reads. |
| `top_up_bond(caller, sla_id, amount)` | `caller` must equal the SLA's `provider` | Adds `amount` of the SLA's token to the tracked balance; transfers it in from `caller`. |
| `trigger_settlement(caller, sla_id, round_id)` | **none on `caller`** | Deliberately permissionless: `caller` is accepted but never passed to `require_auth`, and is not checked against any role. Anyone believing quorum has formed may call it; nobody's funds move because of who calls it, only because quorum was independently reached. `caller` is not stored and is not part of the `SettlementPaid` event; it is visible only as an argument of the transaction itself. Reads the round tally from the configured `watcher_registry` contract (via `contractimport!`, not the source crate) and requires `tally.votes_down >= quorum_threshold`, else `QuorumNotMet`. Idempotent per `(sla_id, round_id)`: a second call returns `AlreadySettled` with no transfer. Payout is `min(penalty_per_breach, remaining bond balance)`; if that would be `<= 0`, fails with `BondExhausted` before any transfer. |
| `is_round_settled(sla_id, round_id)` | none | Read. |
| `cancel_sla(caller, sla_id)` | `caller` must equal the SLA's `provider` | Marks the SLA `Cancelled`. Does not touch the bond balance. |
| `withdraw_remaining_bond(caller, sla_id)` | `caller` must equal the SLA's `provider` | Requires the SLA already be `Cancelled` (`SlaNotActive` otherwise); transfers the full remaining balance to `caller` and zeroes it. A balance that is already `0` is rejected with `InvalidAmount` before any transfer, so a repeat call cannot emit a `BondWithdrawn` event with `amount: 0`. This zero-balance rejection was added in commit `99be8a1` (2026-09-28), after the Testnet deployment described in `evidence/testnet-2026-09-27.md` was built; that deployment does not contain it. |
| `pause(caller)` / `unpause(caller)` | `caller` must be admin | Gates `create_sla` only. Existing SLAs can still be settled, cancelled, topped up, or withdrawn while paused; this is deliberate, not an oversight, so a pause cannot be used to strand a provider's or beneficiary's funds. |

### Events (`src/events.rs`)

| Event | Topics | Data |
|---|---|---|
| `SlaCreated` | `sla_id: u64`, `provider: Address` | `token: Address`, `bond_amount: i128`, `beneficiary: Address` |
| `BondToppedUp` | `sla_id: u64` | `amount: i128` |
| `SettlementPaid` | `sla_id: u64`, `round_id: u64` | `payout: i128`, `beneficiary: Address` |
| `SlaCancelled` | `sla_id: u64` | (none) |
| `BondWithdrawn` | `sla_id: u64` | `amount: i128` |

The topic/data split for all five events above has been confirmed against
real emitted Testnet events. `SlaCreated` and `SettlementPaid` were confirmed
first (see `slasettle-hub`'s indexer commit history); `BondToppedUp`,
`SlaCancelled` and `BondWithdrawn` were first observed on 2026-09-27 on the
current Testnet deployment, with raw topics and data fetched directly through
`getEvents`:

| Event | Tx | Raw topics | Raw data |
|---|---|---|---|
| `bond_topped_up` | `50d35d47794844b90cf167102bd518db675c2cf9b6a42ccabbcb5f709c72ba60` | `["bond_topped_up","0"]` | `{"amount":"5000000"}` |
| `sla_cancelled` | `25fb9d95f34c9f155bd039cbcd80e5ab88eec7049067bab415d74bd4eadda6e8` | `["sla_cancelled","1"]` | `{}` |
| `bond_withdrawn` | `f2d1be379addfe39a2ab0fc6f3933be1e55b00c01dd3b85d1b104741a06f3173` | `["bond_withdrawn","1"]` | `{"amount":"20000000"}` |

Source: `evidence/testnet-2026-09-27.md`. The same three events were fetched
again on 2026-09-29 (a fresh read-only `getEvents` against the same
deployment, decoded with the indexer's own `decodeEvent`) and matched:
`slasettle-hub/evidence/parity-matrix-2026-09-29.md`, section 4. As with the
registry events, topic 0 is the snake_case event name and the `Topics` column
lists only the topics after it. The raw values above are printed as recorded in
the evidence file, where `u64` and `i128` values were rendered as decimal
strings; the indexer's decoder receives them as `bigint`.

### Fixed: `quorum_threshold == 0`

`trigger_settlement` checks `tally.votes_down < config.quorum_threshold`.
`votes_down` is a `u32` and can never be less than zero, so a
`quorum_threshold` of zero made that comparison always false: settlement
would pass with zero watcher votes, defeating the quorum mechanism entirely
for that SLA. `create_sla` now rejects `quorum_threshold == 0` at creation
time.

### Error codes are numbered per contract

Both contracts number their errors from `1`, and the numbers collide with
different meanings: `#3` is `NotAWatcher` in `watcher_registry` but
`SlaNotActive` in `sla_vault`; `#4` is `DuplicateCheck` versus
`AlreadySettled`; `#5` is `ContractPaused` versus `QuorumNotMet`. A failed
call surfaces on the wire as `Error(Contract, #N)`, so `N` is only meaningful
together with the contract that raised it.

### Round IDs

`round_id` is an opaque `u64` supplied by the caller of `submit_check` and
`trigger_settlement`. Neither contract reads the ledger clock (`env.ledger()`
is not used anywhere) and neither validates `round_id` against real time, so
a registered watcher can vote on any `round_id` and anyone can call
`trigger_settlement` for any `round_id`. The convention used by the off-chain
code in `slasettle-hub` is `floor(unix_seconds / ROUND_LENGTH_SECONDS)` with a
default of 60 seconds: the watcher daemon computes it from its own system
clock, and the indexer computes it from the latest ledger's close time. That
convention lives entirely off-chain.

### Cross-contract call

`trigger_settlement` reads the round tally through
`soroban_sdk::contractimport!` against the compiled `watcher_registry.wasm`
interface, not by linking the `watcher_registry` source crate. Linking the
source crate as a regular dependency previously caused a linker failure
(duplicate `initialize`/`pause`/`unpause` symbols in the combined WASM), which
is why the interface is imported rather than the crate depended on directly
in non-test code. `watcher_registry` remains a `dev-dependency` used directly
by the test suite, which exercises the real cross-contract call rather than a
mock.
