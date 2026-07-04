# Bridge API Contract

This document defines the private bridge HTTP contract used by the MIoT/Xiaomi
and Matter bridge executors.

Machine-readable schemas are provided under:

```text
docs/schemas/miot-bridge-request.schema.json
docs/schemas/matter-bridge-request.schema.json
docs/schemas/bridge-response.schema.json
```

Executor tests load these schemas and compare them against serialized
`MiotBridgeRequest` and `MatterBridgeRequest` values, so request-shape drift is
caught before release.

The bridge is intentionally outside this public repository. EdgeHome Harness
owns verified internal commands and bridge request generation. The private
bridge owns vendor credentials, device-specific identifiers, protocol calls,
controller state, and real-device validation.

```text
MiniCPM candidate JSON
  -> Rust validation / registry / gate
  -> ExecutionPlan
  -> Bridge request
  -> Private bridge
  -> Vendor/controller/device
```

## Security Boundary

The model must never emit bridge routes, vendor identifiers, tokens, URLs, or
controller details. The harness reads only trusted registry/config values and
posts a typed bridge request after dry-run evidence already exists.

Bridge base URLs must be:

```text
http:// or https://
parseable as URLs
host-bearing
without query strings
without fragments
without username/password userinfo
```

Examples:

```text
valid:   http://127.0.0.1:8787
valid:   https://bridge.example.test/api
invalid: https://bridge.example.test?token=secret
invalid: https://user:pass@bridge.example.test
invalid: mqtt://bridge.example.test
```

Secrets are supplied through environment variables such as
`EDGEHOME_MIOT_BRIDGE_TOKEN` or `EDGEHOME_MATTER_BRIDGE_TOKEN`; they must not be
stored in public configs, prompts, traces, or docs.

## Shared HTTP Shape

Both bridge executors use:

```text
POST <base_url>/<path>
Authorization: Bearer <private bridge token>
Content-Type: application/json
Accept: application/json
```

The public harness treats any non-2xx response as a failed execution. Empty
2xx bodies are accepted as `{}`. Non-empty 2xx bodies must be valid JSON.

Backend responses and error bodies are sanitized before they are surfaced or
stored as executor evidence. Token-like fields, private URLs, Xiaomi
identifiers, Matter fabric/node/endpoint fields, oversized strings, oversized
arrays, and overly deep objects are redacted or bounded.

## MIoT / Xiaomi Endpoint

Path:

```text
/v1/miot/execute
```

Request:

```json
{
  "protocol": "miot",
  "route_id": "miot.bedroom_ac",
  "device_id": "bedroom_air_conditioner",
  "action": "set_temperature",
  "method": "set_properties",
  "arguments": {
    "temperature": 24
  }
}
```

The private MIoT bridge maps `route_id` to private Xiaomi data such as:

```text
did
siid
piid
aiid
local token
cloud token
LAN endpoint
state readback method
```

Those fields must not appear in public configs, model prompts, or returned
executor evidence. If the bridge returns them, EdgeHome Harness will redact
known sensitive keys before trace storage, but bridge implementations should
avoid returning them in the first place.

Minimum success response:

```json
{
  "ok": true,
  "route_id": "miot.bedroom_ac",
  "state": "accepted"
}
```

Recommended failure response:

```json
{
  "ok": false,
  "error_code": "device_unreachable",
  "message": "device did not acknowledge within timeout"
}
```

Use HTTP status codes to signal execution failure:

```text
400 invalid request or unsupported action
401/403 invalid bridge token
404 unknown route_id
409 unsafe or conflicting device state
422 supported route but unsupported arguments
502/504 vendor/controller timeout
```

## Matter Endpoint

Path:

```text
/v1/matter/execute
```

Request:

```json
{
  "protocol": "matter",
  "route_id": "matter.hallway_light",
  "device_id": "hallway_light",
  "action": "set_brightness",
  "command": "level_control.move_to_level",
  "arguments": {
    "level_pct": 30
  }
}
```

The private Matter bridge maps `route_id` to controller data such as:

```text
fabric
node_id
endpoint_id
cluster
attribute
controller session
commissioning state
state readback method
```

Those fields belong to the private controller bridge, not to MiniCPM and not to
the public harness.

Minimum success response:

```json
{
  "ok": true,
  "route_id": "matter.hallway_light",
  "state": "accepted"
}
```

Recommended failure response:

```json
{
  "ok": false,
  "error_code": "controller_timeout",
  "message": "Matter controller did not complete the command"
}
```

Use HTTP status codes to signal execution failure:

```text
400 invalid request or unsupported command
401/403 invalid bridge token
404 unknown route_id
409 unsafe or conflicting controller state
422 supported route but unsupported arguments
502/504 controller/device timeout
```

## Bridge Implementation Requirements

A private bridge should provide:

```text
route_id -> real device/controller mapping
per-route capability validation
secret isolation
request authentication
idempotency or duplicate handling where possible
state readback where the backend supports it
bounded JSON responses
no raw secret or vendor identifier echo
structured errors
operator logs outside the public repo
```

It should not:

```text
trust model-provided vendor identifiers
accept direct natural-language input
let query strings carry tokens
return private credentials or controller internals
silently execute unsupported actions
pretend success when the vendor/controller times out
```

## Harness Claim Boundary

Allowed public claim:

```text
EdgeHome Harness implements MIoT/Xiaomi and Matter bridge request adapters with
guarded opt-in bridge execution paths.
```

Not allowed without private validation evidence:

```text
Universal Xiaomi support
Embedded Matter controller
Matter fabric commissioning support
Real-device production deployment
Default real-device execution
```
