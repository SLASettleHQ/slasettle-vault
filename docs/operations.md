# Testnet Operations

This guide describes the existing v0.2 Testnet contracts and the safe order for future authorized maintenance. It does not authorize or record a new deployment or transaction.

## Current GREEN contracts

The public v0.2 GREEN deployment is recorded in [Testnet evidence dated 2026-10-26](../evidence/testnet-v0.2-2026-10-26.md):

| Contract | Testnet ID | Deployed WASM SHA-256 |
| --- | --- | --- |
| `watcher_registry` | `CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY` | `e433b7e36580889b1a2aa94570ed7adee7e9048d4d211a10031ffcfa8fc53171` |
| `sla_vault` | `CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2` | `23ea6cdc28a11883fd5979c06fcdea3b3f68a5b2aa9f0bace855de0149d8ac79` |

The v0.2 pair was deployed and cut over while Testnet used Protocol 29. The dated evidence proves that deployment and lifecycle only; check the live Testnet RPC before stating a current protocol version. The Hub [v0.2 cutover runbook](https://github.com/SLASettleHQ/slasettle-hub/blob/main/apps/docs/developers/v0.2-cutover.md) contains the coordinated Web, indexer, watcher, and rollback record.

## Toolchain and reproducible build checks

The workspace pins `soroban-sdk` to `29.0.1` in `Cargo.toml` and `Cargo.lock`. That SDK requires Rust `1.91.0` or newer. CI uses the rolling Rust `stable` toolchain with `rustfmt`, `clippy`, and the `wasm32v1-none` target; it does not pin an exact Rust compiler release. CI installs Stellar CLI `28.1.0`. The CLI version is a separate constraint from the Soroban SDK version.

`Cargo.lock` fixes the Rust dependency versions, but the rolling Rust toolchain means the lockfile alone does not pin every input to byte-for-byte WASM output. For a reproducibility check, use the exact source revision named in the evidence, record the actual Rust, Cargo, and Stellar CLI versions, and compare both output hashes with the recorded GREEN artifacts. From the Vault repository root:

```bash
rustup target add wasm32v1-none
rustc --version
cargo --version
stellar --version
stellar contract build
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features
sha256sum target/wasm32v1-none/release/watcher_registry.wasm
sha256sum target/wasm32v1-none/release/sla_vault.wasm
```

CI builds the contracts before Rust checks because `sla_vault` imports the registry's compiled WASM interface. Compare fresh builds with the GREEN hashes above and the dated evidence before describing bytecode parity. A successful local build is not a new Testnet deployment check. Do not substitute historical BLUE hashes for the current GREEN pair.

## Deployment and initialization order

The v0.2 deployment is already present. Its evidence records Protocol 29 at deployment and cutover. Protocol 30 compatibility is unverified. Before any future deployment, confirm the live Testnet protocol and that the selected CLI and SDK support it, then use an explicitly authorized disposable Testnet account. Follow these dependency steps and record each resulting address and transaction from actual output:

1. Build both WASMs and verify their hashes and generated interfaces.
2. Deploy `watcher_registry`, then initialize it with the intended administrator.
3. Register the watcher addresses that are authorized to vote. Confirm the registered count and ownership before setting the vault quorum.
4. Deploy `sla_vault`, then initialize it with the same intended administrator and the initialized registry address.
5. Verify the contract IDs, deployed interfaces, administrators, registry binding, watcher count, pause states, and network passphrase before using the pair. For example, the read-only CLI interface check is `stellar contract info interface --id <CONTRACT_ID> --network testnet`; verify state values through supported contract reads and the live Testnet RPC.

Initialization is one-time. If a deployment or initialization result is uncertain, read the transaction and contract state before retrying. Do not infer an address or transaction from a planned command.

## Watcher and administrator responsibilities

The registry administrator owns watcher registration and removal. Adding the same watcher twice is a no-op; removing an absent watcher is a no-op. Removing watchers changes the count used to validate new SLA quorum thresholds. Existing SLAs keep their stored quorum, so check active SLAs before reducing the eligible set: a set smaller than an SLA's quorum cannot produce enough votes for settlement. Current GREEN watcher identities are project-controlled, as described in the evidence.

Watchers must derive the current round from `get_current_round`, check the stored SLA endpoint commitment, submit the matching endpoint hash, and avoid resubmitting for an SLA round. A watcher submission is a signed assertion, not proof of an HTTP observation.

## Pause and unpause behavior

Only the configured administrator can pause or unpause either contract.

- Registry pause rejects new `submit_check` calls. Reads and watcher registration/removal remain available.
- Vault pause rejects `create_sla`. Existing settlement, top-up, cancellation, and withdrawal paths remain available.

Record the reason, operator, affected contract, and read-back result for any pause action. Do not promise that pausing either contract freezes all activity.

## TTL monitoring and maintenance

Both contracts use `INSTANCE_TTL_EXTEND_TO = 518_400` and `INSTANCE_TTL_THRESHOLD = 501_120`. Persistent entries use the same thresholds. The approximate 30-day extension assumes about five seconds per ledger; ledger count is the configured unit.

The public `extend_contract_ttl()` entrypoint extends the live contract instance and code TTL without an administrator signature. State-changing functions also extend the persistent entries they touch. This entrypoint does not renew every watcher, check, tally, SLA, balance, or settlement entry. Reads do not renew those persistent entries. Monitor relevant entry TTLs and the Testnet's current ledger behavior instead of treating one maintenance call as a blanket renewal.

## Archival and restoration

An archived contract instance and an archived persistent entry are distinct restore targets. The Soroban CLI provides separate procedures for [restoring a contract instance](https://developers.stellar.org/docs/tools/cli/cookbook/restore-contract-instance) and [restoring contract storage](https://developers.stellar.org/docs/tools/cli/cookbook/restore-contract-storage). Persistent keys such as `(sla_id, round_id, endpoint_hash)` require the exact key XDR when using the CLI restore command.

The public `extend_contract_ttl()` function is not a restore transaction and cannot be relied on to recover archived state. The repository's existing evidence does not establish that archival restoration was successfully executed for these contracts. Treat restoration as unverified until a separately authorized Testnet restore is observed and the restored entries are read back. If data has been evicted or required key XDR is unavailable, this repository does not provide a business-state reconstruction path.

## Troubleshooting and recovery boundaries

| Symptom | Safe read-only checks | Recovery boundary |
| --- | --- | --- |
| Wrong network or protocol | Call Testnet RPC `getNetwork`; compare passphrase and protocol to the run being inspected. | Do not submit a transaction until the selected network is confirmed. |
| `AlreadyInitialized` | Read the contract's initialized state and address. | Do not retry initialization or infer that another deployment is equivalent. |
| `NotAWatcher` or `DuplicateCheck` | Read watcher membership and `has_watcher_voted(sla_id, round_id, watcher)`. | The admin owns membership changes; a duplicate cannot be replaced by another hash. |
| `InvalidRound` | Read `get_current_round`; verify the requested round is current or immediately previous. | Wait for a valid round. The 60-second schedule is fixed. |
| `QuorumNotMet` | Read the SLA's `endpoint_hash` and query the tally with that exact hash and round. | Do not treat a tally for another endpoint as evidence for this SLA. |
| Contract or persistent entry is archived | Inspect the exact contract ID and entry key. Use the CLI restore procedure for the right durability/key. | Restore needs an authorized funded Testnet source, fees, exact key XDR for structured entries, and post-restore read-back. No successful restore is claimed here. |
| Local WASM hash differs | Recheck toolchain, lockfile, build order, and the recorded GREEN hash. | Stop before deployment; do not overwrite or redeploy the existing contracts as a troubleshooting shortcut. |

For past recovery context, see [recovery evidence dated 2026-09-28](../evidence/recovery-2026-09-28.md). Preserve all dated deployment and lifecycle evidence; append new evidence rather than rewriting old observations.
