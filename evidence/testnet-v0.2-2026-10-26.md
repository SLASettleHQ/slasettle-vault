# Current v0.2 Testnet deployment evidence, 2026-10-26

This concise record identifies the Vault artifacts used by the current public SLASettle GREEN stack. Detailed Web, indexer, watcher, and lifecycle proof remains in the Hub repository and is linked below rather than duplicated here.

## Deployment identity

- Network: Stellar Testnet only.
- Network state: Protocol 29 at deployment/cutover verification. This does not assert that Testnet will remain on Protocol 29.
- Build dependency: `soroban-sdk 29.0.1`.
- `watcher_registry`: `CDTIVQ6I77EMFP42522RIFKMSMJID3BDLNR5LPLJEGVAS4Q2DJDMN3DY`.
- `sla_vault`: `CA5R7V5IGMYOBRSAMP4R5MWURSNTY6565NSKEEVR6626PLC3VM2A3EZ2`.
- `watcher_registry` WASM SHA-256: `e433b7e36580889b1a2aa94570ed7adee7e9048d4d211a10031ffcfa8fc53171`.
- `sla_vault` WASM SHA-256: `23ea6cdc28a11883fd5979c06fcdea3b3f68a5b2aa9f0bace855de0149d8ac79`.
- Shipped contract-code release: [`v0.2.0`](https://github.com/SLASettleHQ/slasettle-vault/releases/tag/v0.2.0), targeting `371ca2e66adb1c161ed967d3676ae4e0f1f0a419`.

Fresh documentation-reconciliation validation rebuilt both release WASMs from that unchanged contract source, reproduced the hashes above, and passed all 72 Vault tests. No contract was redeployed and the `v0.2.0` tag was not moved.

## Scope and trust boundary

- This deployment is Testnet only and has not undergone an independent third-party security audit.
- Current verification uses three project-controlled Testnet watcher identities; it does not demonstrate an independent third-party committee.
- Endpoint commitment binds votes to an endpoint commitment and keeps different endpoint tallies isolated. It does not cryptographically prove that a watcher performed or truthfully reported an HTTP observation.
- The watcher registry is shared globally across SLAs, and watcher voting does not use commit-reveal.

## Detailed production evidence

- [Hub v0.2 production-cutover evidence](https://github.com/SLASettleHQ/slasettle-hub/blob/main/evidence/v0.2-production-cutover-2026-10-26.md)
- [Hub current Testnet deployment and lifecycle record](https://github.com/SLASettleHQ/slasettle-hub/blob/main/apps/docs/testnet-deployment.md)
- [Hub v0.2 cutover record](https://github.com/SLASettleHQ/slasettle-hub/blob/main/apps/docs/developers/v0.2-cutover.md)

The Protocol 28 BLUE/v0.1 pair remains live only as rollback/legacy infrastructure. Its dated transaction evidence is preserved in [`testnet-2026-10-01.md`](./testnet-2026-10-01.md).
