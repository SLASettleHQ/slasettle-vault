<div align="center">

<img src="assets/slasettle-vault-banner.webp" alt="SLASettle Vault Banner" width="100%" />

# SLASettle Vault

Soroban smart contracts for bonded service-level agreements on Stellar.

[![CI](https://github.com/SLASettleHQ/slasettle-vault/actions/workflows/ci.yml/badge.svg)](https://github.com/SLASettleHQ/slasettle-vault/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Stellar Testnet](https://img.shields.io/badge/Stellar-Testnet-black.svg)](https://stellar.org)
[![Soroban SDK](https://img.shields.io/badge/Soroban_SDK-29.0.1-blue.svg)](https://soroban.stellar.org)
[![WASM Parity](https://img.shields.io/badge/WASM_Parity-Verified-brightgreen.svg)](evidence/testnet-v0.2-2026-10-26.md)
[![Branch Protected](https://img.shields.io/badge/Branch_Protection-Active-success.svg)](https://github.com/SLASettleHQ/slasettle-vault/tree/main)

[Documentation](https://slasettle-docs.vercel.app) · [SLASettle Hub](https://github.com/SLASettleHQ/slasettle-hub) · [Live App](https://slasettle-web.vercel.app) · [Testnet Contracts](#currently-deployed-on-testnet) · [Deployment Evidence](evidence/testnet-v0.2-2026-10-26.md) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md)

</div>

---

## What is SLASettle Vault?

SLASettle Vault contains the Soroban smart contracts that power the SLASettle protocol on Stellar. It provides the on-chain financial guarantees and verification logic needed to hold service providers accountable to their uptime commitments:

- **`watcher_registry`**: Manages an authorized watcher set and tallies votes on whether monitored service endpoints were operational during discrete observation rounds.
- **`sla_vault`**: Escrows provider bond deposits, evaluates watcher consensus against configured SLA thresholds, and disburses penalty payouts to designated beneficiaries when downtime breaches are confirmed.

## Why it exists

Traditional SLAs rely on retroactive manual claims processes, proprietary provider dashboards, and opaque outage calculations. Providers hold all customer funds while customers bear the burden of proving service failure.

SLASettle replaces manual claims with on-chain guarantees:
- Providers lock real token bonds before agreements take effect.
- Registered watcher nodes monitor endpoint availability round by round. Watcher software can be operated independently; the current Testnet committee is project-controlled.
- Breach penalties are paid out directly from the bond when watcher consensus confirms a failure.
- Contract rules are public, deterministic, and verifiable on Stellar.

## Current Testnet status

| Item | Details | Status |
|---|---|---|
| Target Network | Stellar Testnet (`https://soroban-testnet.stellar.org`) | Connected |
| Stellar Protocol | Protocol 29 at v0.2 deployment/cutover verification | Verified at deployment; Testnet may upgrade |
| Soroban SDK | 29.0.1 | Pinned |
| `watcher_registry` | `CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY` | [Explorer](https://stellar.expert/explorer/testnet/contract/CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY) |
| `sla_vault` | `CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2` | [Explorer](https://stellar.expert/explorer/testnet/contract/CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2) |
| WASM Bytecode Parity | Local build matches deployed bytecode byte-for-byte | Verified (Phase 8) |
| Automated Vault Tests | 72 passing workspace unit and cross-contract tests | Passing |
| Third-Party Audit | Formal external security audit | Not performed |

## Contracts

### `watcher_registry`

Tracks an eligible set of watchers and records their binary votes (up or down) on endpoint health per round.
- Does not enforce quorum thresholds; it only counts votes.
- Rejects duplicate votes from the same watcher in the same round.
- Requires admin authentication for registration, removal, and pausing.
- Authoritative for watcher membership and vote counts.

### `sla_vault`

Holds the provider's token bond and evaluates breach conditions.
- Calls `watcher_registry` to read raw vote tallies.
- Enforces quorum: if the down-vote count meets or exceeds the required threshold, the round is judged breached.
- Disburses a fixed penalty amount to the beneficiary, capped at the remaining bond balance.
- Rejects zero-balance bond withdrawals when an SLA has no active balance remaining.

## How settlement works

```
Observation Round
      │
      ▼
Watchers check endpoint ───► submit_check (watcher_registry)
                                      │
                                      ▼
Breach detected? ──────────► trigger_settlement (sla_vault)
                                      │
                                      ▼
                        Reads tally from watcher_registry
                                      │
                                      ▼
                        Quorum reached?
                         ├── YES ──► Pay penalty to beneficiary
                         └── NO  ──► No payout
```

Settlement is permissionless: any caller can submit `trigger_settlement` for an eligible round. Neither repository triggers settlement automatically; it must be invoked by an operator, beneficiary, or automated cron once a round closes.

## Architecture

```mermaid
flowchart TD
    Provider["Service Provider"] -->|"create_sla (deposits bond)"| SLAVault["sla_vault (Soroban)"]
    Provider -->|"top_up_bond / cancel_sla"| SLAVault
    Watchers["Registered Watchers"] -->|"submit_check (round health)"| WatcherRegistry["watcher_registry (Soroban)"]
    SLAVault -->|"get_round_tally / is_watcher"| WatcherRegistry
    Beneficiary["SLA Beneficiary"]
    Caller["Any Caller (Permissionless)"] -->|"trigger_settlement"| SLAVault
    SLAVault -->|"Penalty payout on breach"| Beneficiary
    Provider -->|"withdraw_remaining_bond"| SLAVault
```

## Verification

| Verification Class | Scope | Evidence |
|---|---|---|
| v0.2 WASM Bytecode Parity | Byte-for-byte match between fresh local builds and deployed GREEN hashes | [v0.2 Testnet evidence](evidence/testnet-v0.2-2026-10-26.md) |
| v0.2 Production Lifecycle | Create, top-up, quorum, settlement, cancellation, and withdrawal | [Hub production-cutover evidence](https://github.com/SLASettleHQ/slasettle-hub/blob/main/evidence/v0.2-production-cutover-2026-10-26.md) |
| Historical BLUE Settlement | On-chain settlement payout on the legacy pair | [Testnet evidence 2026-10-01](evidence/testnet-2026-10-01.md) |
| Automated Tests | 72 unit and cross-contract integration tests | `cargo test --workspace` |

## Quick start

### Prerequisites

- Rust stable (1.91.0+ required by `soroban-sdk` 29.0.1)
- `wasm32v1-none` target
- `stellar-cli` 28.1.0+

```bash
rustup target add wasm32v1-none
stellar contract build
```

The workspace builds two WASM artifacts:
- `target/wasm32v1-none/release/watcher_registry.wasm`
- `target/wasm32v1-none/release/sla_vault.wasm`

A committed `Cargo.lock` guarantees reproducible WASM compilation hashes.

## Testing

```bash
cargo test --workspace
```

The test suite covers:
- Success-path tests for all contract entrypoints.
- Error-variant tests for all defined custom errors.
- Real cross-contract invocations between `sla_vault` and `watcher_registry` (no mocked interfaces).

## Deploying (testnet)

Deployment order matters: `sla_vault` requires `watcher_registry`'s address upon initialization.

```bash
# 1. Deploy and initialize watcher_registry
REGISTRY_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/watcher_registry.wasm \
  --source-account <ADMIN> --network testnet)

stellar contract invoke --id $REGISTRY_ID --source-account <ADMIN> \
  --network testnet -- initialize --admin <ADMIN>

# 2. Register initial watchers
stellar contract invoke --id $REGISTRY_ID --source-account <ADMIN> \
  --network testnet -- register_watcher --caller <ADMIN> --watcher <WATCHER_ADDRESS>

# 3. Deploy and initialize sla_vault pointing at the registry
VAULT_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/sla_vault.wasm \
  --source-account <ADMIN> --network testnet)

stellar contract invoke --id $VAULT_ID --source-account <ADMIN> \
  --network testnet -- initialize --admin <ADMIN> --watcher_registry $REGISTRY_ID
```

### Currently deployed on Testnet

| Contract | ID | Explorer |
|---|---|---|
| `watcher_registry` | `CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY` | [View on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY) |
| `sla_vault` | `CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2` | [View on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2) |

Authoritative v0.2 WASM hashes (`soroban-sdk` 29.0.1; Protocol 29 at deployment/cutover verification):
- `watcher_registry`: `e433b7e36580889b1a2aa94570ed7adee7e9048d4d211a10031ffcfa8fc53171`
- `sla_vault`: `23ea6cdc28a11883fd5979c06fcdea3b3f68a5b2aa9f0bace855de0149d8ac79`

The current record is [evidence/testnet-v0.2-2026-10-26.md](evidence/testnet-v0.2-2026-10-26.md). The BLUE/v0.1 contracts remain live as rollback/legacy infrastructure but are not the current public stack. Their transaction history is preserved in [evidence/testnet-2026-10-01.md](evidence/testnet-2026-10-01.md).

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request against `main`:
- `stellar contract build`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo fmt --check`

Live `main` protection requires pull requests and the strict, up-to-date `check, test, build` CI status check. Administrator enforcement is enabled. Force pushes and branch deletion are disabled. Required approving reviews are currently set to 0.

Dependabot (`.github/dependabot.yml`) checks `cargo` and `github-actions` weekly.

## Security

See [`SECURITY.md`](SECURITY.md).

**This project has not undergone an independent third-party security audit.**

- The authorization model enforces role-based checks on sensitive admin and provider operations (`require_auth`).
- `trigger_settlement` is deliberately permissionless so any party can execute a valid settlement.
- Known security limitations are documented below.

## Known limitations

1. **No commit-reveal vote protection**: Contract state is public; a late-voting watcher could observe earlier votes and copy the majority. A commit-reveal scheme is planned for a future upgrade.
2. **Display-only uptime target**: `uptime_target_bps` is stored for agreement display; settlement is per-round based on fixed penalty amounts, not automated monthly aggregate calculations.
3. **Shared watcher set**: A single registry-wide watcher set serves all SLAs in this deployment; provider-curated watcher sets are not supported in v0.2.
4. **Operational TTL extension**: Contracts do not extend instance storage lifetimes automatically in code; rent/TTL must be monitored and renewed operationally.

Full interface specifications and authorization rules are detailed in [`SLASettle-contract-spec.md`](SLASettle-contract-spec.md).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

This project is licensed under the [MIT License](LICENSE).
