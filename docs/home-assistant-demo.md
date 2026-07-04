# Home Assistant Demo

This document explains how EdgeHome Harness connects to Home Assistant as a
device backend for demos and local verification.

The boundary is deliberate:

```text
Home Assistant owns device integration.
EdgeHome Harness owns the small-model command safety pipeline.
The model never sees Home Assistant tokens, URLs, API paths, or entity IDs.
The model never outputs Home Assistant service-call JSON directly.
Real execution is disabled by default.
```

## Current Status

Implemented:

```text
HomeAssistantConfig
SecretsLoader
HomeAssistantClient
HomeAssistantExecutor
dry-run service-call translation
entity_id route validation
opt-in REST service execution
optional post-state fetch
redacted executor evidence
local HTTP fixture tests
CLI execute integration test against a local fixture
```

Primary implementation:

```text
crates/edgehome-executor/src/home_assistant.rs
crates/edgehome-cli/src/main.rs
```

Example config:

```text
configs/home_assistant.yaml.example
```

## Claim Boundary

Safe public claim:

```text
EdgeHome Harness implements a Home Assistant gateway boundary: verified internal
ExecutionPlan values can be translated into Home Assistant service-call
payloads, checked in dry-run mode, and executed only through explicit private
configuration.
```

Do not claim:

```text
EdgeHome Harness replaces Home Assistant.
EdgeHome Harness supports every Home Assistant domain.
EdgeHome Harness has been validated against every device integration.
The model can output Home Assistant entity IDs or service payloads.
Real Home Assistant execution is enabled by default.
```

## Device Registry Mapping

The registry owns the route from an internal device to a Home Assistant entity:

```yaml
devices:
  - device_id: living_room_main_light
    aliases: ["living room light", "main living room light"]
    room: living_room
    device_type: light
    backend: home_assistant
    backend_entity_id: light.living_room
    risk_level: low
```

The model may propose:

```json
{
  "intent": "control_device",
  "room": "living_room",
  "device_alias": "living room light",
  "device_type": "light",
  "action": "turn_on"
}
```

The model must not propose:

```text
light.living_room
/api/services/light/turn_on
Home Assistant base URL
Home Assistant token
Authorization header
```

## Backend Readiness Check

Run a read-only check:

```powershell
cargo run -q -p edgehome-cli -- backend check --backend home_assistant --registry configs/devices.home_assistant.example.yaml
```

Expected public example behavior:

```text
dry_run_ready = true
execute_enabled = false
execute_ready = false
real_execution_default = disabled
```

This command validates config shape, route count, route syntax, execution
switches, and secret availability. It does not contact Home Assistant and does
not touch devices.

## Dry-Run Translation

Example user request:

```text
Turn on the living room light.
```

Verified internal plan:

```json
{
  "target": "living_room_main_light",
  "action": "turn_on",
  "params": {
    "brightness": null,
    "temperature": null,
    "mode": null
  }
}
```

Home Assistant dry-run payload:

```json
{
  "backend": "home_assistant",
  "service": "light.turn_on",
  "service_path": "/api/services/light/turn_on",
  "entity_id": "light.living_room",
  "payload": {
    "entity_id": "light.living_room"
  }
}
```

This is still not real execution. It is a deterministic adapter payload derived
from a verified `ExecutionPlan`.

## Private Execute Config

Copy the example config to a private path outside the repository:

```powershell
New-Item -ItemType Directory -Force "$HOME\.edgehome" | Out-Null
Copy-Item configs\home_assistant.yaml.example "$HOME\.edgehome\home_assistant.private.yaml"
```

Use either an environment variable:

```powershell
$env:EDGEHOME_HA_TOKEN = "your-long-lived-access-token"
```

or a token file outside the repository:

```yaml
token_file: C:\Users\you\.edgehome\ha-token.txt
```

Private config must explicitly opt in:

```yaml
execute_enabled: true
```

Public example configs must keep:

```yaml
execute_enabled: false
```

## Explicit Execute Flow

First produce a dry-run trace:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite --config-dir configs dry-run --mock "Turn on the living room light"
```

Then execute the returned trace only if the private backend config is ready:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite --config-dir configs execute <trace_id> --confirm --backend-config "$HOME\.edgehome\home_assistant.private.yaml"
```

Execution rules:

```text
The CLI loads the existing DryRunPlan from trace evidence.
The CLI does not parse fresh natural language during execute.
Traces older than 600 seconds are rejected before backend calls.
Traces that already completed real execution are rejected.
Risk and confirmation checks still apply.
Backend/config/transport failures are recorded as redacted failure evidence.
Only successful execution marks a trace as completed.
```

## Evidence And Redaction

Successful execution records a redacted `ExecutorResponse` evidence item.

For Home Assistant:

```text
service response body is sanitized before storage
post-state fetch stores entity_id/state/timestamps summary
full attributes are not stored
tokens are not stored
Authorization headers are not stored
```

Failed explicit execution records:

```text
ExecutorResponse evidence with success = false
real_execute_failed step
real_execution_failed audit event
sanitized failure reason
```

Failed attempts do not count as completed real execution, so a fresh trace can
be retried after the private config is fixed.

## Verified Locally

Current tests cover the Home Assistant boundary without requiring a real Home
Assistant instance:

```text
example_config_loads_with_execution_disabled
gateway_route_validation_rejects_invalid_entity_id
executor_dry_run_does_not_require_token_or_call_real_device
executor_execute_is_disabled_by_default
executor_execute_posts_service_and_fetches_redacted_state_summary
executor_execute_redacts_home_assistant_error_body
execute_trace_posts_home_assistant_gateway_from_private_config
execute_trace_records_failed_backend_execution_without_completing_trace
```

Run the focused checks:

```powershell
cargo test -p edgehome-executor home_assistant --locked
cargo test -p edgehome-cli execute_trace_posts_home_assistant_gateway_from_private_config --locked
cargo test -p edgehome-cli execute_trace_records_failed_backend_execution_without_completing_trace --locked
```

Run the full release gate:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1
```

## Real Home Assistant Demo Checklist

Before using a real Home Assistant instance:

```text
1. Use a non-critical test device or helper entity.
2. Put the token outside the repository.
3. Keep the private config outside the repository.
4. Confirm backend check passes.
5. Run dry-run first and inspect the service payload.
6. Use execute only with --confirm.
7. Inspect trace export after execution.
8. Revoke the token after public demos if needed.
```

Good demo target:

```text
light.test_lamp
input_boolean.edgehome_demo
switch.demo_plug_with_no_load
```

Avoid:

```text
locks
alarms
gas devices
cameras
garage doors
high-power plugs
anything safety-critical
```

## Troubleshooting

`execute_enabled_false`

The private config still has `execute_enabled: false`. This is the correct
default. Set it to true only for explicit local tests.

`secret_available = false`

The configured token environment variable or token file is missing. Do not put
tokens into committed config files.

`invalid entity_id`

The registry route is not a valid Home Assistant `domain.object_id` entity ID.
Fix `backend_entity_id` in the private registry or example registry.

`stale dry-run trace`

The dry-run trace is older than 600 seconds. Create a new dry-run trace before
execute.

`already been executed`

The trace already completed real execution. Create a new dry-run trace before a
new real execution attempt.

## Public Positioning

Use this sentence:

```text
The Home Assistant path demonstrates the adapter boundary: verified internal
commands can be translated into Home Assistant service-call payloads, checked in
dry-run mode, and executed only through explicit, private, auditable
configuration.
```
