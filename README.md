# slasettle-vault

Two Soroban contracts: `watcher_registry` and `sla_vault`. Together they let
a Stellar-based service back its uptime promise with a real bond, checked by
watchers (independent of the provider by design), and paid out when enough of
them have voted the service down and someone calls `trigger_settlement`. Nothing
in either repository calls it automatically; on the current Testnet deployment
the watcher addresses were registered by the project admin for evidence runs,
not by independent operators.

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

Requires a current stable Rust and `soroban-sdk` 28.x. `soroban-sdk` was
bumped from 27.0.6 to 28.0.0 by a merged Dependabot PR
(SLASettleHQ/slasettle-vault#1); CI (which runs on `dtolnay/rust-toolchain@stable`)
passed on that PR and on `main` afterward, so a current stable Rust is
confirmed sufficient. The exact minimum `rust-version` for 28.0.0 has not
been reconfirmed as precisely as it was for 27.0.6 (which declared
`rust-version = "1.91.0"`, verified by testing 1.84.0/1.85.0/1.91.0
directly against this dependency graph); 1.91.0 was not reverified against
28.0.0 specifically. If you hit a toolchain error building this
repository, check `soroban-sdk`'s current declared `rust-version` against
your installed Rust rather than assuming either number in this paragraph
still applies.

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

**`soroban-sdk` is currently 28.0.0** (merged via Dependabot PR #1;
`cargo test --workspace` still passes 46/46, `stellar contract build`
still succeeds with zero linker errors, both confirmed in CI and locally).
This is a real, important gap to be aware of: the Testnet contracts
described below under "Currently deployed" were built and deployed with
`soroban-sdk` 27.0.6, **before** this upgrade. Rebuilding from the current
`main` produces a different `sla_vault.wasm` (hash
`951f28b5a9edd705eaa03f17ada24f768e2942ca703abb1ac3c2f83a8b12ee5a` in the CI
log of commit `8d9c517`, rustc 1.98.1; it was `2f958b86…` at `8449bd7`, and
it changes with any source edit, including doc comments, because doc strings
are embedded in the contract spec) than what is actually live on Testnet (hash
`6909713244bf5837954b8d584343e2136bd7570a10da8db7b30533e613b67830`). The
currently deployed contracts have not been redeployed against the
28.0.0 build, and the live Testnet evidence recorded in this repository
was gathered against the 27.0.6 build. Do not assume the two are
equivalent without redeploying and re-verifying.

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

This was a fresh deployment as of 2026-09-27, built from WASM hash
`6909713244bf5837954b8d584343e2136bd7570a10da8db7b30533e613b67830` for
`sla_vault` (`soroban-sdk` 27.0.6), which includes the `quorum_threshold
== 0` fix. **It no longer matches the current `main`**, which has since
moved to `soroban-sdk` 28.0.0 and builds a different `sla_vault.wasm` hash
(see "Building" above). It also predates the zero-balance rejection in
`withdraw_remaining_bond` (commit `99be8a1`, 2026-09-28): on the live
contract a repeat withdrawal from an already-empty bond still succeeds as a
no-op and emits a `BondWithdrawn` event with `amount: 0`, which is exactly the
behavior recorded in `evidence/testnet-2026-09-27.md`. Both contracts are
initialized, with 5 watchers registered. On 2026-09-29 the on-chain WASM of
both contracts was re-fetched and its SHA-256 matched the hashes above, and a
build of the current source exposes an identical public interface apart from
one omitted storage-key type (see `slasettle-hub`'s
`evidence/parity-matrix-2026-09-29.md`, section 6). Full live evidence, including the quorum-zero rejection
confirmed against this exact deployment, real watcher votes, a real
settlement, cancellation, withdrawal, and pause behavior, is in
`evidence/testnet-2026-09-27.md`; that evidence is valid for the 27.0.6
build it was gathered against, not automatically for the current 28.0.0
source.

The previously deployed pair below predates the quorum fix (and also
predates the SDK bump) and is kept as historical evidence only, not as
verification of the current source:

```
watcher_registry (historical): CBEZ3XBIWK2AWYGZRNDGNZG3AZTJHFMQL5HVWTEUZZ5HLSCO4QDFJB77
sla_vault (historical):        CBA4DFNUBVCPLEAUD5O2CHSUB6DRWUNM7A537EBVPAGDETFBB2CABXI2
```

- `create_sla`: `258c86d2a0de481d60240dd29cea6de490840bd29f78e550fb97fb4fb8028b7c`
- `trigger_settlement` (payout): `b1dc301a22f8381ee9705a72e214d212e1f1c81c9b0ac53729506708b286d85e`

## Continuous integration and dependency maintenance

`.github/workflows/ci.yml` runs on every push to `main` and every pull
request against it: `cargo check`, `cargo test`, `cargo clippy`, and
`stellar contract build`, plus an informational (non-blocking) `cargo fmt
--check`. `main` is currently green.

Dependabot is configured (`.github/dependabot.yml`) for the `cargo` and
`github-actions` ecosystems, weekly. It has already opened and this
project has already merged real dependency PRs, including the
`soroban-sdk` 27.0.6 to 28.0.0 bump described above.

`main` is branch-protected: pull requests are required, the CI job above
(`check, test, build`) is a required status check, force pushes and branch
deletion are disabled. Required approving reviews are set to 0, since this
is currently a solo-maintained repository; that is a deliberate choice for
the current maintainer count, not an oversight. Protection is not enforced
for repository administrators (`enforce_admins` is off, read from the
branch-protection API on 2026-09-29), so an administrator can push to `main`
directly.

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
