# P4-J7-11 Role route and attempt admission baseline

## Scope

P4-J7-11 records the existing provider route/admission boundary: the server-owned role/profile
selects a route, and each model attempt is bound to a prepared call, opaque permit, budget and
authority/config epoch before send.

## Implemented source slice

- Role catalog/model profile and provider catalog route resolution are separate from prompt text.
- `RouteDecision` is retained in `kiana-domain/src/versioning.rs`; prepared calls and permits live
  in `kiana-domain/src/model.rs` and the provider gateway is `kiana-provider/src/lib.rs`.
- Prepared model calls carry request/route identity; permit validation binds attempt/request hash,
  expiry and route digest before send.
- Budget/attempt admission and unknown/expired permit outcomes are represented as structured
  deny paths; provider source remains the only model gateway.

## Evidence boundary

GitHub Actions is the test authority for the source guard. Local tests are intentionally not run.
This step does not claim live provider transport, external requests, durable attempt settlement or
physical model effects.
