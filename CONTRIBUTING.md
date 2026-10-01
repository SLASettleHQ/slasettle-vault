# Contributing to slasettle-vault

## Prerequisites

- A current stable Rust toolchain, plus `clippy` and `rustfmt` components,
  and the `wasm32v1-none` target:

  ```bash
  rustup target add wasm32v1-none
  rustup component add clippy rustfmt
  ```

  See `README.md`'s "Building" section for the exact minimum verified
  against this repository's dependency graph, and re-verify it yourself
  if you hit a toolchain error; it changes as `soroban-sdk` is upgraded.

- The [Stellar CLI](https://github.com/stellar/stellar-cli). CI installs
  28.1.0 specifically (matching the Soroban SDK 28 major version).
  deployment); a newer local install will generally still work for
  building and testing, but verify against a real deployment before
  relying on it for anything Testnet-facing.

## Install, build, test

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features
stellar contract build
```

These are the same commands CI runs (`.github/workflows/ci.yml`).
`cargo fmt --check` is also run in CI but is currently informational
only (`continue-on-error`), because of pre-existing formatting drift
that predates the workflow; running `cargo fmt` yourself on lines you
actually touch is still good practice, just don't feel obligated to
reformat lines you didn't otherwise change.

## Repository structure

```
contracts/watcher_registry   tracks eligible watchers, counts votes
contracts/sla_vault           holds bonds, decides settlement
evidence/                     dated, real Testnet/security/recovery evidence
SLASettle-contract-spec.md    the interface/authorization reference
SECURITY.md                   security scope, reporting, known limitations
```

Every function should have at least one success-path test and one test
per distinct error variant it can return, matching the existing pattern
in `contracts/*/src/test.rs`. `trigger_settlement`'s tests deploy both
contracts and exercise the real cross-contract call rather than mocking
it; follow that pattern for anything else that crosses the two contracts.

## Branch workflow and pull requests

`main` is branch-protected: pull requests are required, and the CI check
named `check, test, build` must pass before merging. Force pushes and
branch deletion are disabled.

```text
branch
→ make your change
→ run the commands above locally
→ commit (see Commit conventions below)
→ push your branch
→ open a pull request
→ CI runs automatically
→ merge once CI passes
```

Required approving reviews are currently set to 0, since this repository
is solo-maintained; that may change if it stops being solo-maintained,
but it is not something a contributor should try to work around by
finding a reviewer who doesn't actually understand the change.

## Commit conventions

- One genuine logical unit of work per commit. Don't split one change
  into several commits to look more active, and don't bundle unrelated
  changes into one commit to save time.
- Stage specific files (`git add <file> <file>`), never `git add .`.
  Review `git diff --cached` before committing.
- Push a genuine commit immediately after making it; don't let real,
  finished work sit unpushed.
- Write commit messages that explain why a change was made, not just
  what changed, especially for a fix: what was the actual bug, how was
  it found (a failing test, a real Testnet observation, a security
  review), and what evidence confirms the fix actually works.

## Security-sensitive code

Anything touching authorization checks (`require_auth`, the
admin/provider/watcher boundary checks), token transfers, or the
quorum/settlement logic in `sla_vault` is security-sensitive. Changes
here should:

- explain in the commit message exactly what boundary or invariant the
  change affects
- include a regression test that would have failed before the fix, not
  just a test that happens to pass after it
- be cross-checked against `SECURITY.md` and, where relevant, against
  the real Testnet evidence in `evidence/` rather than local tests alone

See `SECURITY.md` for how to report a vulnerability; there is currently
no private reporting channel, which `SECURITY.md` states plainly rather
than inventing one.

## Documentation expectations

If a change affects a claim in `README.md`, `SECURITY.md`, or
`SLASettle-contract-spec.md`, update that document in the same change,
not as a follow-up "someday." Don't state something as verified unless
you actually ran the command or observed the real Testnet result that
verifies it; use the same honest status vocabulary this repository's
evidence files already use (`VERIFIED`, `TESTED LOCALLY`,
`LOGICALLY COVERED`, `UNVERIFIED`, `KNOWN LIMITATION`, `BLOCKED`) rather
than a stronger claim than the evidence supports.
