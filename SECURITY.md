# Security

`slasettle-vault` contains two Soroban contracts, `watcher_registry` and `sla_vault`.
Both are deployed to Stellar Testnet only. **No independent third-party security
audit has been performed on this code.** Everything in this document reflects an
internal engineering review by the people building this repository, not an
external audit.

## Scope

- `contracts/watcher_registry`
- `contracts/sla_vault`

The related `SLASettleHQ/slasettle-hub` repository (indexer, watcher daemon,
SDK, frontend) has its own security posture and is out of scope for this
document.

## Reporting a vulnerability

This repository does not currently have GitHub's private vulnerability
reporting feature enabled, and no dedicated security contact address exists
yet. If you find a vulnerability, please open a GitHub issue with the minimum
detail needed to establish that a real issue exists (do not include a full
exploit in a public issue) and ask for a private channel to share the rest.
We will follow up from there. This is a real gap for a project at this stage,
not a deliberate choice, and it should be closed before any production
deployment.

## Authorization model, as implemented

This section describes actual code behavior, not intent. Line references are
to the current `main` branch.

### `watcher_registry`

- `initialize`: caller must be the given `admin` address (`admin.require_auth()`).
  Can only run once; storage already holding `DataKey::Admin` causes it to
  fail.
- `register_watcher` / `remove_watcher` / `pause` / `unpause`: caller must be
  the stored admin (`require_admin`, which itself calls
  `caller.require_auth()`).
- `submit_check`: caller must be the `watcher` address whose vote it is
  (`watcher.require_auth()`), and that address must already be registered.
  Duplicate votes for the same `(sla_id, round_id, watcher)` are rejected
  outright, not silently overwritten.
- `is_watcher`, `get_watcher_count`, `get_round_tally`, `has_watcher_voted`:
  unauthenticated reads, by design. Nothing sensitive is returned.

### `sla_vault`

- `initialize`: caller must be the given `admin` address. One-time only.
- `create_sla`: caller must be the `provider` address creating the SLA
  (`provider.require_auth()`). The provider's bond is transferred from the
  provider's own account in the same call, so no separate approval step
  exists in this version.
- `top_up_bond`, `cancel_sla`, `withdraw_remaining_bond`: caller must
  authenticate as `caller` and must equal the SLA's stored `provider`.
- `trigger_settlement`: **deliberately permissionless.** `caller` is accepted
  as a parameter but is never passed to `require_auth()`, and is not checked
  against any role. This is intentional: nobody's funds move because of the
  call itself, only because quorum was independently reached, so anyone
  believing quorum has formed may trigger the payout. `caller` is recorded
  only for observability and is not otherwise used.
- `pause`, `unpause`: caller must be the stored admin.
- `get_sla`, `get_bond_balance`, `is_round_settled`: unauthenticated reads.

## Money movement

- `create_sla` transfers `bond_amount` of the given token from `provider` into
  the vault contract's own balance, in the same call as record creation.
- `trigger_settlement` transfers `min(penalty_per_breach, remaining bond
  balance)` from the vault to `beneficiary`, decrements the tracked balance
  by the same amount, and marks the round settled, all before emitting the
  `SettlementPaid` event. Settlement for a given `(sla_id, round_id)` pair can
  only happen once; a second call returns `Error::AlreadySettled` without any
  transfer.
- `withdraw_remaining_bond` requires the SLA be `Cancelled` first, and
  transfers the full remaining tracked balance to the provider, then zeroes
  it.
- Every token transfer uses the Soroban SAC client (`token::Client`) and
  relies on the transfer call itself failing (propagating an error, aborting
  the transaction) if the token contract rejects it. There is no explicit
  balance check before a transfer beyond the `bond_amount`/payout arithmetic
  already described.

## Known limitations (real, not fixed here)

These are documented in `README.md` and remain true as of this review:

- `watcher_registry` has no commit-reveal scheme. A watcher who submits late
  can see how earlier watchers voted before submitting, and could copy the
  emerging majority instead of reporting what they actually observed.
- `uptime_target_bps` on `SLAConfig` is stored for display only. Nothing in
  either contract computes or enforces an aggregate uptime percentage over a
  billing period; settlement is purely per-round.
- Watchers are one shared, global set across every deployment of
  `watcher_registry` on a given contract instance; a provider cannot curate
  their own trusted watcher set in this version.

## Fixed in this review

- `create_sla` previously accepted `quorum_threshold == 0` with no
  validation. Because `trigger_settlement` compares
  `tally.votes_down < config.quorum_threshold` and `votes_down` is an
  unsigned integer, a zero threshold made that comparison always false,
  letting settlement pass with zero watcher votes and defeating the quorum
  mechanism entirely for that SLA. `create_sla` now rejects
  `quorum_threshold == 0` with `Error::InvalidAmount`. Verified against a
  real deployed contract on Testnet, not just the local regression test:
  see `evidence/testnet-2026-09-27.md`.
- `withdraw_remaining_bond` previously succeeded as a silent no-op when
  the bond balance was already zero: no funds moved (the transfer was
  already guarded by `balance > 0`), but the call was still accepted and
  still emitted a `BondWithdrawn` event with `amount: 0`, wasting a real
  transaction fee and emitting a misleading event. Now rejects with
  `Error::InvalidAmount` when the balance is already zero, consistent with
  `create_sla`'s and `top_up_bond`'s existing zero-value checks. This was
  not a fund-safety issue at any point; see
  `evidence/recovery-2026-09-28.md` for the full investigation.

## Dependency and secret scanning

`cargo audit` was run against this workspace on 2026-09-28
(cargo-audit 0.22.2). Result: one non-CVE warning, `paste` v1.0.15 is
unmaintained (RUSTSEC-2024-0436). Traced with `cargo tree -i paste`: it is
pulled in transitively through `soroban-env-host` (part of `soroban-sdk`
itself), not a dependency this repository chose or can remove directly.
No automated, ongoing scanner (a scheduled CI job, Dependabot security
updates) is configured; the scan above was a real, manually-run, one-time
check, not continuous coverage.

A manual secret scan (`git grep` for Stellar secret key patterns, PEM
private-key blocks, and generic API-key patterns) was run across every
commit in this repository's complete reachable history on 2026-09-28.
Nothing was found. No dedicated entropy-based secret-scanning tool has
been run successfully against this repository as of this date. See
`evidence/security-review-2026-09-28.md` for the full review this section
summarizes.

## Supported versions

Testnet only. There is no versioned release yet, and no mainnet deployment.
This document will be updated once a release process and a mainnet target
exist.
