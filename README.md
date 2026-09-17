# slasettle-vault

Two Soroban contracts: `watcher_registry` and `sla_vault`. Together they let
a Stellar-based service back its uptime promise with a real bond, verified by
independent watchers, and paid out automatically when enough of them agree
the service was down.

Testnet only, for now. Nothing here has been audited, and there is a known,
unresolved trust gap in `watcher_registry` — see below.

## What each contract does

`watcher_registry` tracks an eligible set of watchers and counts their votes
on whether an endpoint was up or down for a given round. It has no concept of
a quorum threshold and never will — it only counts.

`sla_vault` holds a provider's bonded funds and decides what counts as a
confirmed breach. It calls `watcher_registry` to read the raw vote count, then
applies its own judgment: if enough watchers voted down, it pays a fixed
penalty to the beneficiary, capped at whatever is left in the bond.

## Building

```
rustup target add wasm32v1-none
stellar contract build
```

Requires Rust 1.84+ and `soroban-sdk` 27.x. If a newer stable release of
either exists by the time you're reading this, verify it and use it, the way
this repo's own contract spec was written against a freshly checked version
rather than an assumed one.

**This code has not been compiled in the environment it was written in.**
That environment's toolchain was capped at Rust 1.75 with no reachable path
to upgrade it, and `soroban-sdk` 27's dependency tree requires Cargo's
`edition2024` feature (Rust 1.85+). Run `cargo check` and `cargo test` in a
real environment before trusting or deploying any of this.

## Testing

```
cargo test
```

Every function has at least one success-path test and one test per distinct
error variant it can return. `trigger_settlement`'s tests deploy both
contracts and exercise the real cross-contract call, not a mock.

## Deploying (testnet)

Order matters — `sla_vault` depends on `watcher_registry`'s address.

```bash
# 1. Deploy and initialize the registry first.
REGISTRY_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/watcher_registry.wasm \
  --source-account <ADMIN> --network testnet)
stellar contract invoke --id $REGISTRY_ID --source-account <ADMIN> \
  --network testnet -- initialize --admin <ADMIN>

# 2. Register the initial watcher set.
stellar contract invoke --id $REGISTRY_ID --source-account <ADMIN> \
  --network testnet -- register_watcher --caller <ADMIN> --watcher <WATCHER_1>
# ...repeat per watcher

# 3. Deploy and initialize the vault, pointing it at the registry.
VAULT_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/sla_vault.wasm \
  --source-account <ADMIN> --network testnet)
stellar contract invoke --id $VAULT_ID --source-account <ADMIN> \
  --network testnet -- initialize --admin <ADMIN> --watcher_registry $REGISTRY_ID

echo "watcher_registry: $REGISTRY_ID"
echo "sla_vault: $VAULT_ID"
```

## Known limitations, stated plainly

1. **No defense against last-mover vote copying.** Contract state is public,
   so a watcher who votes late can see how others voted first and simply
   copy the majority instead of reporting what they actually observed. The
   real fix is a commit-reveal scheme; this version ships without it.
2. **`uptime_target_bps` is display-only.** Settlement is per-round, driven
   by a fixed penalty, not an enforced monthly aggregate percentage.
3. **One shared watcher set** across every SLA on the deployment. A provider
   cannot curate their own trusted watchers in this version.

Full reasoning for each is in `SLASettle-contract-spec.md`.
