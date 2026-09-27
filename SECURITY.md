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
  `quorum_threshold == 0` with `Error::InvalidAmount`. See the commit fixing
  this for the regression test added alongside it.

## Dependency scanning

No automated dependency vulnerability scanner (e.g. `cargo audit`,
Dependabot security updates) is currently configured for this repository.
This is a real gap, tracked as backlog work, not something this review
resolves.

## Supported versions

Testnet only. There is no versioned release yet, and no mainnet deployment.
This document will be updated once a release process and a mainnet target
exist.
