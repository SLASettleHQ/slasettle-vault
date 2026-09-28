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

Requires Rust 1.91.0 or newer and `soroban-sdk` 27.x. This is not a guess:
`soroban-sdk` 27.0.6 declares `rust-version = "1.91.0"`, and this was
confirmed empirically against the actual dependency graph in this
repository. Rust 1.84.0 fails outright (`edition2024` is not stabilized
until 1.85), and Rust 1.85.0 fails with `soroban-sdk@27.0.6 requires rustc
1.91.0`. Rust 1.91.0 checks and builds successfully. If a newer stable
release of Rust or `soroban-sdk` exists by the time you're reading this,
verify it against the real dependency graph rather than assuming either
number above still holds.

`Cargo.lock` is committed. Current official Cargo guidance is "when in
doubt, check Cargo.lock into version control," and this workspace produces
deployable WASM binaries, not a library meant for other Rust crates to
depend on, which is the strongest case for committing it. This was not a
theoretical concern: regenerating the lockfile in this repository changed
`watcher_registry.wasm`'s build hash even though its source did not change
at all, purely from a different transitive dependency resolution. Without a
committed lockfile, the exact same source can produce a different
deployable artifact depending on when it's built.

This code has been compiled and tested in a real environment: `cargo test
--workspace` passes (see below), `cargo check --workspace` passes, and
`stellar contract build` succeeds for both contracts with zero linker
errors, producing deployable WASM for both, all with the committed
lockfile in place. `cargo fmt --check` currently fails and `cargo clippy
--workspace --all-targets --all-features` has 2 pre-existing style
warnings (`needless_borrows_for_generic_args` in `sla_vault`); neither has
been cleaned up as part of this pass, since doing so would touch lines
unrelated to it.

A newer `soroban-sdk` major version (28.0.0) exists on crates.io as of this
writing. This repository intentionally stays on the `27.x` line declared in
`Cargo.toml`, since the deployed Testnet contracts described below were
built against `27.0.6` and a major SDK upgrade has not been verified
against that deployment.

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

### Currently deployed on Testnet

```
watcher_registry: CBKAQETJU3PLB54LJRSA7ZH2ZG4TBQHHDSWZ23R4VVTV7WBIX3QZBUZ6
sla_vault:        CD4FSW2E2YLGNVPQ6T6DA6FKRK735HLMN676IEF2O5LKZYVDYHHDIIFL
```

This is a fresh deployment of the current source, built from WASM hash
`6909713244bf5837954b8d584343e2136bd7570a10da8db7b30533e613b67830` for
`sla_vault`, which includes the `quorum_threshold == 0` fix. Both contracts
are initialized, with 5 watchers registered. Full live evidence, including
the quorum-zero rejection confirmed against this exact deployment, real
watcher votes, a real settlement, cancellation, withdrawal, and pause
behavior, is in `evidence/testnet-2026-09-27.md`.

The previously deployed pair below predates the quorum fix and is kept as
historical evidence only, not as verification of the current source:

```
watcher_registry (historical): CBEZ3XBIWK2AWYGZRNDGNZG3AZTJHFMQL5HVWTEUZZ5HLSCO4QDFJB77
sla_vault (historical):        CBA4DFNUBVCPLEAUD5O2CHSUB6DRWUNM7A537EBVPAGDETFBB2CABXI2
```

- `create_sla`: `258c86d2a0de481d60240dd29cea6de490840bd29f78e550fb97fb4fb8028b7c`
- `trigger_settlement` (payout): `b1dc301a22f8381ee9705a72e214d212e1f1c81c9b0ac53729506708b286d85e`

## Known limitations, stated plainly

1. **No defense against last-mover vote copying.** Contract state is public,
   so a watcher who votes late can see how others voted first and simply
   copy the majority instead of reporting what they actually observed. The
   real fix is a commit-reveal scheme; this version ships without it.
2. **`uptime_target_bps` is display-only.** Settlement is per-round, driven
   by a fixed penalty, not an enforced monthly aggregate percentage.
3. **One shared watcher set** across every SLA on the deployment. A provider
   cannot curate their own trusted watchers in this version.

Full interface reference, authorization rules, and reasoning for each
limitation above is in `SLASettle-contract-spec.md`.
