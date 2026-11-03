# Contract Architecture

SLASettle Vault contains two Soroban contracts. The detailed ABI, storage keys, errors, and event fields remain in [`SLASettle-contract-spec.md`](../SLASettle-contract-spec.md); this page explains ownership, interaction, and trust boundaries without duplicating that specification.

## Contract responsibilities

### `watcher_registry`

The registry owns the administrator, registered watcher set, watcher count, per-watcher checks, endpoint-bound tallies, and its pause flag. A registered watcher authenticates its own `submit_check` call. The registry checks watcher membership, round freshness, duplicate submissions, and pause state. It counts `Up` and `Down` reports but does not interpret quorum or move tokens.

### `sla_vault`

The vault owns SLA configuration, endpoint commitment, provider bond balances, lifecycle status, settlement records, its administrator, and its binding to one registry contract. It validates SLA parameters and token transfers, queries the registry, evaluates quorum, and pays the beneficiary.

## Cross-contract interaction

The vault makes two read calls to the registry:

1. `create_sla` reads `get_watcher_count` and rejects zero watchers or a quorum greater than the registered count.
2. `trigger_settlement` reads `get_round_tally(sla_id, round_id, config.endpoint_hash)` and compares `votes_down` with the SLA's stored quorum.

The registry never calls the vault. It has no token authority and cannot settle an SLA. A tally for the wrong endpoint hash cannot satisfy the vault's query.

The quorum is stored in the SLA at creation. Later watcher removals do not change it. If the eligible watcher set becomes smaller than an existing SLA's quorum, that SLA can no longer collect enough votes to settle a breach.

## Authoritative state and storage

| State | Owner | Storage |
| --- | --- | --- |
| Administrator, watcher count, registry binding, next SLA ID, pause flags | The contract that uses the value | Instance storage |
| Watcher membership, per-watcher check, endpoint-bound tally | `watcher_registry` | Persistent storage |
| SLA configuration, bond balance, settled-round flag | `sla_vault` | Persistent storage |
| Current ledger timestamp and transaction outcome | Stellar ledger / Soroban host | Network state and transaction result |
| HTTP response, endpoint spelling, watcher process health | Off-chain watcher | Not authoritative contract state |

The vault is authoritative for the SLA's endpoint commitment and bond accounting. The registry is authoritative for watcher membership and recorded votes. The indexer and browser are projections and reads of those sources, not alternate contract state.

## Authorization and trust boundaries

- The administrator initializes each contract, registers or removes watchers, and can pause or resume the relevant operations. Admin calls require the administrator's Soroban authorization.
- A watcher must be registered and authenticate its own submission. One watcher can submit only once per `(sla_id, round_id)`, even if it tries a different endpoint hash.
- The provider authenticates SLA creation, top-ups, cancellation, and withdrawal. The provider cannot withdraw the bond while the SLA is active.
- `trigger_settlement` is permissionless. The caller parameter is not authenticated; the contract moves funds only after it verifies the stored SLA, endpoint-bound `DOWN` tally, quorum, and remaining bond.
- The contracts do not perform HTTP requests. An endpoint commitment binds and isolates votes, but cannot prove that a watcher made a real request or reported honestly.

The administrator can change watcher eligibility and pause selected operations. The administrator cannot rewrite an SLA's endpoint, quorum, beneficiary, or settlement record, and has no direct function to withdraw provider bonds.

## Endpoint commitments and tally isolation

Hub SDK and watcher code derive the 32-byte commitment from the domain-separated prefix `slasettle:endpoint:v1:` plus the ASCII-trimmed URL. The URL spelling is otherwise preserved. The contract stores only the hash in `SLAConfig`.

The registry stores each check under `(sla_id, round_id, watcher)` and each tally under `(sla_id, round_id, endpoint_hash)`. A check with a different hash affects only that hash's tally, while still consuming the watcher's one submission for that SLA round. The vault always asks for the commitment recorded in its SLA configuration.

## Lifecycle invariants

- Observation rounds are derived from `ledger.timestamp() / 60`. Their duration is fixed at 60 seconds. The registry accepts the current round and the immediately preceding round.
- SLA creation requires `uptime_target_bps` in `1..=10_000`; the value is descriptive and is not used to calculate aggregate uptime.
- At SLA creation, quorum must be positive and no greater than the current watcher count. Settlement is per round and pays `min(penalty_per_breach, remaining_bond)` once for each `(sla_id, round_id)`.
- Cancellation is one-way. Remaining bond withdrawal requires a cancelled SLA and a positive balance.
- Pausing the registry stops new check submissions while reads and watcher administration continue. Pausing the vault stops new SLA creation; existing settlement, top-up, cancellation, and withdrawal calls remain available.
- Persistent storage entries have individual TTLs. Writes renew the entries they touch; public `extend_contract_ttl` entrypoints extend the contract instance and code TTL, not every persistent entry.

## Failure cases

| Operation | Rejection condition | Effect |
| --- | --- | --- |
| Create an SLA | Invalid bond or penalty amount, uptime target outside `1..=10_000`, zero quorum, quorum above the current watcher count, or vault pause | The SLA is not created. Fix the configuration or have the administrator resume creation before retrying. |
| Submit a check | Registry pause, caller not registered, a duplicate `(sla_id, round_id, watcher)` submission, or a round outside the current/previous-round window | The check is rejected. A consumed duplicate cannot be replaced with another endpoint hash. |
| Trigger settlement | SLA missing or cancelled, round already settled, `DOWN` tally below quorum, eligible watcher set too small to reach the stored quorum, no remaining bond, or a token/registry call failure | No successful settlement is recorded for that invocation. A permissionless caller may retry when the underlying condition permits it; settlement is not scheduled automatically. |
| Read or update stored state | A contract instance or persistent entry has expired or is archived | Calls may fail or require an explicit restore operation. TTL extension does not reconstruct evicted business state. |

The endpoint hash limits which tally the vault reads, but it does not validate a URL or prove that a watcher performed an HTTP request. See the canonical specification for the complete public error and interface reference.

## Known limitations

- Testnet only. No Mainnet deployment or Mainnet support is claimed.
- Testnet watchers are project-controlled, not independent operators. The registry uses one shared watcher set.
- Votes are visible as they are submitted. Commit-reveal is not implemented.
- The uptime target is descriptive; settlement is per-round.
- Persistent TTL values have not been calibrated against production rent costs. A lapsed or archived entry can affect availability and requires an explicit recovery procedure.
- No independent third-party security audit has been performed.
