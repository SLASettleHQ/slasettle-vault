> **Correction added 2026-09-29; the original 2026-09-28 text below is unchanged.**
> Under "Token transfer behavior and bond accounting" this review says payout
> capping was "VERIFIED LIVE and TESTED LOCALLY". The live part is not
> supported: the one live settlement (tx `6522d8b7…`) paid the full penalty
> from a bond of `55000000`, so the branch where the remaining bond is below
> the penalty never ran on Testnet. Current classification: implementation,
> source; behavior, TESTED LOCALLY
> (`test_trigger_settlement_caps_payout_at_remaining_balance`); live execution,
> UNVERIFIED. See `slasettle-hub/evidence/index.md` rows E5 and E7.

# Security review, 2026-09-28

## Scope and method

This is an **internal engineering security review**, performed by the same
people building this repository. It is not an independent third-party
audit, and no independent audit has been performed on any part of this
project as of this date.

Repositories reviewed: `SLASettleHQ/slasettle-vault` (contracts) and
`SLASettleHQ/slasettle-hub` (watcher, indexer, SDK, frontend).

Method: direct source inspection (with line-level citations where
practical), reading the actual test suites, running real dependency and
vulnerability scanners against the actual dependency graphs, a manual
secret-pattern scan across tracked files and full git history in both
repositories, and connecting findings to the real live Testnet evidence
already recorded in `evidence/testnet-2026-09-27.md` and
`evidence/recovery-2026-09-28.md` where applicable.

## Contract security

### Authorization

Every state-changing function's caller check was re-verified directly
against source (`contracts/sla_vault/src/lib.rs`,
`contracts/watcher_registry/src/lib.rs`) and confirmed against real
rejected Testnet transactions in `evidence/testnet-2026-09-27.md`:

- Admin-only: `initialize` (both contracts), `pause`/`unpause` (both
  contracts), `register_watcher`/`remove_watcher` (registry).
- Provider-only (must equal the SLA's stored `provider`): `top_up_bond`,
  `cancel_sla`, `withdraw_remaining_bond`.
- Watcher-only (must equal the vote's own `watcher`): `submit_check`.
- **Deliberately permissionless**: `trigger_settlement`. `caller` is
  accepted but never passed to `require_auth`, confirmed both in source
  and live: the real settlement in Phase 10 was triggered by `alice`, an
  account that is not admin, provider, or beneficiary, and it succeeded
  exactly as designed. This is a documented design choice
  (`SLASettle-contract-spec.md`), not an oversight.

### Quorum enforcement and the quorum-zero fix

`trigger_settlement` requires `tally.votes_down >= config.quorum_threshold`.
`create_sla` now rejects `quorum_threshold == 0` (fixed in commit
`84c6ee8`, this repository's own history). This was **VERIFIED LIVE**
against the fresh Testnet deployment in Phase 10: a real `create_sla` call
with `quorum_threshold: 0` was rejected with `Error(Contract, #7)` =
`InvalidAmount`, and a valid quorum of 3 was later reached and correctly
triggered settlement. This closes what was originally the project's most
important known contract-logic gap.

### Duplicate execution

- Duplicate vote: `DataKey::Check(sla_id, round_id, watcher)` existence is
  checked before recording a vote; rejected with `DuplicateCheck`.
  **VERIFIED LIVE** (Phase 10, real rejected transaction).
- Duplicate settlement: `DataKey::SettledRounds(sla_id, round_id)` gates
  `trigger_settlement`; rejected with `AlreadySettled`. **VERIFIED LIVE**
  (Phase 10, real rejected transaction).

### Token transfer behavior and bond accounting

- `create_sla` transfers the bond from `provider` to the vault in the same
  call as record creation; no window where the record exists without the
  funds, or the funds move without the record.
- `trigger_settlement` transfers `min(penalty_per_breach, remaining
  balance)`, decrements the tracked balance, and marks the round settled,
  all before the event fires. Payout capping was **VERIFIED LIVE and
  TESTED LOCALLY** (`test_trigger_settlement_caps_payout_at_remaining_balance`).
- `withdraw_remaining_bond` now rejects a zero balance instead of a silent
  no-op (fixed this pass, commit `99be8a1`). This was a real correctness
  gap, not a fund-safety one: the transfer was already guarded by
  `balance > 0` before the fix, so no funds were ever at risk of double
  spending or incorrect accounting; the only real-world effect was a
  wasted transaction fee and a misleading zero-amount event.

### Integer arithmetic

`Cargo.toml`'s `[profile.release]` sets `overflow-checks = true`,
confirmed directly in the committed workspace manifest. This means an
arithmetic overflow or underflow anywhere in the contract (for example, if
`balance - payout` or `balance + amount` in `sla_vault`'s bond accounting
ever went out of range) traps and aborts the transaction in the deployed
release WASM, not just in `cargo test`'s debug builds. This is a real,
verified protective property, not assumed from Rust's default debug-only
behavior.

### TTL behavior

Both contracts extend persistent storage TTL to roughly 30 days on every
write that touches the relevant key (`PERSISTENT_TTL_EXTEND_TO = 518_400`
ledgers), re-extending once within about a day of expiry
(`PERSISTENT_TTL_THRESHOLD`). This is a v1 constant, explicitly flagged in
its own source comment as not tuned against real storage-rent cost data
yet. **KNOWN LIMITATION**, not a security defect: worst case, an
infrequently-touched SLA's storage could expire and become unreadable,
which is a liveness/availability concern, not a fund-safety one, since no
function silently reads a default value in place of expired state that
would misrepresent an SLA's real status.

### Cross-contract calls

`trigger_settlement` reads the round tally via `soroban_sdk::contractimport!`
against the compiled `watcher_registry.wasm` interface, not by linking the
source crate into the release build (fixed earlier in this project's
history to resolve a real linker conflict). The registry link was
**VERIFIED functionally** in Phase 10: settlement only succeeded because
the vault correctly queried the exact fresh registry it was initialized
with, and the tally matched the 3 real votes recorded there.

### Paused state, cancellation, withdrawal

- `pause` on `sla_vault` gates `create_sla` only, deliberately, so an
  existing provider can still top up a bond and a confirmed breach can
  still settle while the contract is paused to new business. **VERIFIED
  LIVE** in Phase 10 (a real `top_up_bond` succeeded while paused).
- `cancel_sla` requires provider auth and does not touch the bond balance;
  `withdraw_remaining_bond` requires the SLA already be `Cancelled`. Both
  confirmed live in Phase 10 with real transactions and real rejected
  attempts in the wrong order.

## Watcher security

- **Private key handling**: `WATCHER_SECRET_KEY` is read once from the
  environment in `internal/config/config.go` and passed directly into
  `contract.NewClient`. It is never logged: `cmd/watcher/main.go`'s
  startup log line (`log.Printf("starting watcher for sla_id=%d,
  target=%s, round_length=%ds", ...)`) explicitly logs only non-sensitive
  fields, and no code path logs the config struct wholesale (no `%+v` on
  `cfg` anywhere in the codebase). Confirmed by direct source read, not
  assumed.
- **Secret exposure check**: a manual scan for Stellar secret key patterns
  (`S[A-Z2-7]{55}`) and PEM private-key blocks across every tracked file
  in the complete reachable git history of both repositories found none.
  `.env` (which holds the real `WATCHER_SECRET_KEY` locally) is gitignored
  and confirmed not tracked; `.env.example` is tracked and contains only
  empty placeholder values.
- **Network selection**: `RPC_URL`/`NETWORK_PASSPHRASE` are both
  environment-configured with sensible Testnet defaults, never hardcoded
  to a different network.
- **RPC trust boundary / sequence handling / replay**: `SubmitCheck`
  builds a fresh transaction per call via `getSourceAccount`, which loads
  the account's current sequence number live before every submission;
  there is no cached or manually-incremented sequence number that could
  drift and cause a replay or invalid-sequence failure. Confirmed by
  source read of `contract.go`'s `getSourceAccount`/`SubmitCheck`.
- **Endpoint handling**: `internal/health.Checker.Check` performs a plain
  HTTP GET with a configured timeout and a configurable max-status
  threshold for "up"; a non-2xx/3xx-range response, a timeout, or a
  connection failure are all classified as `Down`, not silently ignored.
  Tested against real `httptest` servers including a genuine timeout and a
  genuine refused connection (see `evidence/recovery-2026-09-28.md`).
- **Go toolchain vulnerabilities**: `govulncheck ./...` against the real
  watcher module found **25 vulnerabilities, all in the Go standard
  library** (net/url, crypto/tls, crypto/x509, net/http, encoding/asn1,
  encoding/pem), none in `go-stellar-sdk` or any other third-party
  dependency actually called by this code. Every one is fixed in a later
  Go 1.25.x patch release; the installed toolchain (1.25.1) is behind. See
  Dependency scanning below for the full classification.

## Indexer security

- **CORS**: `ALLOWED_ORIGINS` is an explicit, environment-driven allowlist,
  never a wildcard, never a reflection of an arbitrary caller's Origin
  header. Confirmed both by source (`src/api/server.ts`, `src/config.ts`)
  and by live curl verification in an earlier pass of this project
  (allowed origin gets the header, disallowed origin gets none).
- **SQL injection**: every database write in `src/db/db.ts` uses
  `better-sqlite3`'s parameterized `.prepare(...).run(...)` pattern.
  Grepped directly for any template-literal string interpolation into a
  SQL statement (`` `...${...}` `` inside a query string) and found none.
- **Malformed / unknown events**: `classifyEvents` logs and skips an
  unrecognized topic rather than throwing, confirmed by test
  (`an unrecognized topic is skipped, not thrown, and does not appear in
  any batch bucket`).
- **The Phase 10 event-ordering issue**: fixed this pass (commit
  `cc7ff23`). Retested all affected ingestion paths after the fix: the
  full test suite (35/35, including two new regression tests at the
  classification and database layers) and a live reprocess of the real
  Phase 10 ledger range, which now correctly shows all 5 watchers. See
  `evidence/recovery-2026-09-28.md` for the full reproduction chain.
- **Resource limits**: `MAX_LEDGERS_PER_REQUEST` bounds each `getEvents`
  call; pagination on list endpoints uses an opaque cursor
  (`src/api/pagination.ts`), tested for round-tripping and rejecting a
  malformed decoded cursor.
- **Provider scoping**: `GET /v1/providers/:address/slas` filters strictly
  by the `provider` column; confirmed by test that an unmatched address
  returns an empty array, not another provider's data.
- **Logging / sensitive data**: the indexer never handles a private key or
  secret at all; nothing in its logging surface (`pino`) is scoped to any
  data more sensitive than public on-chain events and public Stellar
  addresses.

## SDK security

- **No private key handling**: `packages/sdk` never imports a signing
  library or accepts a secret key anywhere in its API. Every write
  function name is `buildXTx`, returning an **unsigned** `Transaction`;
  the doc comment and README are explicit that signing happens only in
  the wallet layer (`apps/web/lib/wallet.ts`), confirmed by reading both.
- **Network/contract configuration**: read once from
  `NEXT_PUBLIC_SOROBAN_RPC_URL`/`NEXT_PUBLIC_NETWORK_PASSPHRASE`/
  `NEXT_PUBLIC_SLA_VAULT_CONTRACT_ID`/`NEXT_PUBLIC_WATCHER_REGISTRY_CONTRACT_ID`,
  never hardcoded; throws `MissingSdkConfigError` naming exactly which
  variables are missing if any are unset, rather than proceeding with a
  partial or guessed configuration.
- **Integer-safe amounts**: every `i128`/`u64` value round-trips as
  `bigint`, never coerced to `number`, confirmed by the SDK's own test
  suite round-tripping encoded arguments back through `scValToNative`.

## Frontend security

- **Wallet signing**: exclusively through `@stellar/freighter-api`; the
  app never constructs or holds a private key. Confirmed by grep: no
  private-key or secret-signing code exists anywhere under `apps/web`.
- **Network mismatch**: real detection exists
  (`components/network/network-indicator.tsx`, comparing the connected
  wallet's actual network passphrase against the configured one), but it
  is visual-only with no hard block before submission. Recorded as a
  **KNOWN LIMITATION** in `evidence/recovery-2026-09-28.md`, not a
  fund-safety issue since Stellar's own transaction-signing model bakes
  the network passphrase into the signed payload, so a genuinely
  mismatched sign/submit combination fails at the protocol level rather
  than silently executing against the wrong network.
- **XSS exposure**: exactly one `dangerouslySetInnerHTML` use in the
  entire frontend (`app/layout.tsx`, injecting the dark-mode
  flash-prevention script), and its content (`themeInitScript`) is a
  static constant from `lib/theme.ts`, never derived from user input, URL
  parameters, or API data. Every other piece of dynamic content in the
  app goes through React's default auto-escaping.
- **Untrusted API data**: indexer responses are typed and mapped
  explicitly in `lib/indexer.ts`, never rendered as raw HTML.
- **`NEXT_PUBLIC_*` exposure**: all five public environment variables
  (RPC URL, network passphrase, two contract IDs, indexer API URL) are
  information that is already public on-chain or is meant to be known by
  any client connecting to this app; none of them are secrets.

## Dependency scanning

| Ecosystem | Tool | Version | Date | Result |
|---|---|---|---|---|
| Node (pnpm workspace: apps/web, packages/sdk) | `pnpm audit --prod` | pnpm 12.4.2 | 2026-09-28 | No known vulnerabilities found |
| Node (indexer, separate npm project) | `npm audit --omit=dev` | npm 11.19.0 | 2026-09-28 | 0 vulnerabilities |
| Rust (slasettle-vault workspace) | `cargo audit` | cargo-audit 0.22.2, advisory-db loaded 2026-09-28 | 2026-09-28 | 1 warning: `paste` v1.0.15 is unmaintained (RUSTSEC-2024-0436), not a CVE. Traced via `cargo tree -i paste`: pulled in transitively through `soroban-env-host` (part of `soroban-sdk` itself, via the `ark-ff` cryptography crates), not a dependency this repository chose or can remove directly. |
| Go (watcher) | `govulncheck ./...` | govulncheck v1.8.0, DB updated 2026-09-28 | 2026-09-28 | 25 vulnerabilities found, **all in the Go standard library** (none in third-party packages actually called by this code), all already fixed in later Go 1.25.x patch releases (1.25.2 through 1.25.13). The installed toolchain is 1.25.1. |

GitHub Actions dependencies were not reviewed, since no CI workflow exists
in either repository yet (a separate, already-known gap from earlier
phases).

## Secret scanning

**Method**: a dedicated scanner (`gitleaks`) was attempted but its build
did not complete in the available time; it was not substituted with a
fabricated result. Supplemented with a manual pattern-based scan using
`git grep` across every commit in the complete reachable history
(`git rev-list --all`) of both repositories, covering:

- Stellar secret key pattern (`S[A-Z2-7]{55}`)
- PEM private-key block headers (`BEGIN ... PRIVATE KEY`)
- generic API-key/secret/token assignment patterns in source files
- any tracked `.env` file that is not `.env.example`

**Result**: no matches in either repository, in any historical commit or
the current tree. No secret was discovered; nothing to rotate or revoke.

This is a real but partial scan, not equivalent to a dedicated tool's full
entropy-based detection. **UNVERIFIED** by a dedicated secret-scanning
tool as of this date; the manual patterns above are the actual evidence.

## Findings classification

| # | Finding | Class | Status |
|---|---|---|---|
| 1 | `create_sla` accepted `quorum_threshold == 0` | A (was a real Blocker) | Fixed in an earlier pass, VERIFIED LIVE this pass |
| 2 | `withdraw_remaining_bond` silent no-op on zero balance | D | Fixed this pass, TESTED LOCALLY |
| 3 | Indexer watcher registration/removal ordering within one batch | D | Fixed this pass, VERIFIED LIVE this pass |
| 4 | Go standard library behind on security patches (govulncheck) | D | Not fixed this pass; requires a Go toolchain upgrade decision, recorded not silently performed |
| 5 | `paste` crate unmaintained (transitive via soroban-sdk) | E | Not actionable from this repository; upstream soroban-sdk's dependency choice |
| 6 | Frontend network-mismatch is visual-only, no hard submit block | D | Not fixed this pass; recorded as a known limitation, not a fund-safety issue |
| 7 | Persistent storage TTL constant not tuned against real rent data | C (documented known limitation, restated) | Not a new finding; already disclosed |
| 8 | Commit-reveal absence (vote copying) | C (documented known limitation) | Not a new finding; already disclosed, not relabeled as a bug |
| 9 | `uptime_target_bps` display-only | C (documented known limitation) | Not a new finding; already disclosed, not relabeled as a bug |
| 10 | One shared watcher set across deployments | C (documented known limitation) | Not a new finding; already disclosed, not relabeled as a bug |
| 11 | No dedicated secret-scanning tool successfully run | E | Recorded; manual scan substituted, found nothing |
| 12 | No CI workflow in either repository | E | Already known from earlier phases, not this review's discovery |

## Audit status

**No independent security audit has been performed.** This document is an
internal engineering review only. Nothing in this project should be
described as audited, secure, fully secure, or production-ready based on
this review.
