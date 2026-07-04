# MIoT / Xiaomi Bridge Adapter

The MIoT adapter is implemented as a bridge-request boundary.

```text
MiniCPM candidate JSON
  -> Rust validation / normalization
  -> DeviceRegistry route_id
  -> Gated ExecutionPlan
  -> MIoT bridge request
  -> private MIoT bridge
```

This repository does not claim universal Xiaomi device support. The bridge owns
real Xiaomi credentials, device IDs, MIoT spec mapping, local/cloud protocol
details, execution, and state readback.

## Why Bridge Instead Of Direct Xiaomi JSON

Different Xiaomi devices and MIoT profiles can require different identifiers and
operations:

```text
did
siid
piid
aiid
set_properties
action
state readback shape
```

MiniCPM must not emit any of these values. Public prompts and traces should not
contain them either. EdgeHome Harness emits only a trusted route ID and normalized
arguments.

## Registry Route

Example:

```yaml
devices:
  - device_id: bedroom_air_conditioner
    aliases: ["bedroom ac"]
    room: bedroom
    device_type: air_conditioner
    backend: miio_local
    backend_entity_id: miot.bedroom_ac
    risk_level: medium
```

`backend_entity_id` is a private bridge route ID. It is not a MIoT `did`.

## Adapter Config

Example:

```yaml
base_url: http://127.0.0.1:8787
token_env: EDGEHOME_MIOT_BRIDGE_TOKEN
request_timeout_ms: 5000
execute_enabled: false
```

Real execution remains disabled unless `execute_enabled` is explicitly changed
in private config and the bridge token is available.

Read-only readiness check:

```powershell
cargo run -q -p edgehome-cli -- backend check --backend miot --registry configs/devices.miot.example.yaml
```

The check validates route IDs, bridge config shape, execution switch, and token
availability. It does not contact the private MIoT bridge and does not prove
real Xiaomi device support.

CLI shape:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite execute <trace_id> --confirm --backend-config private/miot.yaml
```

The CLI executes only a previously recorded dry-run trace. Public example
configs keep `execute_enabled: false`.

## Golden Request Shape

For:

```text
把卧室空调调到24度
```

The dry-run payload contains:

```json
{
  "backend": "miio_local",
  "protocol": "miot",
  "device_id": "bedroom_air_conditioner",
  "route_id": "miot.bedroom_ac",
  "bridge_path": "/v1/miot/execute",
  "request": {
    "protocol": "miot",
    "route_id": "miot.bedroom_ac",
    "device_id": "bedroom_air_conditioner",
    "action": "set_temperature",
    "method": "set_properties",
    "arguments": {
      "temperature": 24
    }
  }
}
```

The private bridge maps `miot.bedroom_ac` to actual Xiaomi identifiers and calls
the real MIoT/local/cloud implementation.

## Failure Behavior

The MIoT bridge path fails closed for:

```text
missing route
invalid route ID
missing bridge token during real execute
unsupported bridge URL
non-2xx bridge response
execute_enabled = false
policy denied
```

## Tests

Relevant tests:

```powershell
cargo test -p edgehome-executor miot
cargo test -p edgehome-executor dry_run_planner_translates_miot_bridge_payload
```

## Claim Boundary

Allowed public claim:

```text
MIoT/Xiaomi bridge request adapter implemented. Real Xiaomi device support
requires a configured private bridge and device-specific validation.
```

Do not claim:

```text
Universal Xiaomi support
Mi Home replacement
Direct control of all MIoT devices
Real-device validation without bridge logs or hardware evidence
```
