# INT-01 Integration terminology boundary baseline

## Scope

INT-01 fixes the vocabulary and authority boundary for Provider, Connector, MCP, A2A and
Notification. Every integration entry is an input to the existing ControlPlane/Broker path; tool
lists, messages, account metadata and notifications never become authorization evidence by
themselves.

## Implemented source/document slice

- Provider is the model/endpoint route; Connector is the audited external business capability;
  MCP is protocol/transport; A2A is asynchronous task/message ingress; Notification is a derived
  delivery projection.
- Existing docs map each surface to one ControlPlane route and explicitly deny direct Broker,
  model-loop, raw-secret, remote-execution and business-outcome assumptions.
- Current runtime remains local-fixture/stdin-MCP only; HTTP MCP, payment/travel/IoT, enterprise
  tenancy and remote execution remain closed.

## Evidence boundary

GitHub Actions is the test authority for the source/document guard. Local tests are intentionally
not run. This step proves terminology and route-boundary evidence only; it does not claim live
connector/provider/A2A effects or external outcome correctness.
