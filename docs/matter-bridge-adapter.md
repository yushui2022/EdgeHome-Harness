# Matter Bridge Adapter

Matter support is implemented as a controller bridge boundary. EdgeHome Harness
does not embed a full Matter controller.

```text
MiniCPM candidate JSON
  -> Rust validation / normalization
  -> DeviceRegistry route_id
  -> Gated ExecutionPlan
  -> Matter bridge request
  -> private Matter controller bridge
```

The private bridge owns commissioning, fabric membership, node IDs, endpoint
IDs, cluster IDs, command IDs, attributes, and state verification.

## Registry Route

Example:

```yaml
devices:
  - device_id: hallway_light
    aliases: ["hallway light"]
    room: hallway
    device_type: light
    backend: matter_bridge
    backend_entity_id: matter.hallway_light
    risk_level: low
```

`backend_entity_id` is a bridge route ID. It is not a Matter node ID.

## Adapter Config

Example:

```yaml
base_url: http://127.0.0.1:9797
token_env: EDGEHOME_MATTER_BRIDGE_TOKEN
request_timeout_ms: 5000
execute_enabled: false
```

Real execution remains disabled unless explicitly enabled in private config.

CLI shape:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite execute <trace_id> --confirm --backend-config private/matter.yaml
```

The CLI executes only a previously recorded dry-run trace. Public example
configs keep `execute_enabled: false`.

## Golden Request Shape

For:

```text
晚上十点后把走廊灯调到30%
```

The dry-run payload contains:

```json
{
  "backend": "matter_bridge",
  "protocol": "matter",
  "device_id": "hallway_light",
  "route_id": "matter.hallway_light",
  "bridge_path": "/v1/matter/execute",
  "request": {
    "protocol": "matter",
    "route_id": "matter.hallway_light",
    "device_id": "hallway_light",
    "action": "set_brightness",
    "command": "level_control.move_to_level",
    "arguments": {
      "level_pct": 30
    }
  }
}
```

The private Matter bridge maps `matter.hallway_light` to the actual fabric node,
endpoint, cluster, and command.

## Failure Behavior

The Matter bridge path fails closed for:

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
cargo test -p edgehome-executor matter
cargo test -p edgehome-executor dry_run_planner_translates_matter_bridge_payload
```

## Claim Boundary

Allowed public claim:

```text
Matter bridge request adapter implemented. Real Matter control requires a
configured controller bridge.
```

Do not claim:

```text
Embedded full Matter controller
Matter fabric commissioning support
Universal Matter device support
Real-device validation without controller bridge evidence
```
