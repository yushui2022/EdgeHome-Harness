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

The bridge HTTP interface is specified in
[Bridge API Contract](bridge-api-contract.md).

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
token_file:
request_timeout_ms: 5000
execute_enabled: false
```

Real execution remains disabled unless explicitly enabled in private config and
the bridge token is available through `token_env` or a private `token_file`
outside the repository.

Successful controller bridge responses are recorded only after recursive
sanitization. Fabric IDs, node IDs, endpoint IDs, private URLs, token-like
fields, and oversized values are redacted or bounded before executor evidence is
written to trace storage.

Read-only readiness check:

```powershell
cargo run -q -p edgehome-cli -- backend check --backend matter --registry configs/devices.matter.example.yaml
```

The check validates route IDs, bridge config shape, execution switch, and token
availability through either environment or token file configuration. It does
not contact a Matter controller bridge and does not prove real Matter device
control.

CLI shape:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite execute <trace_id> --confirm --backend-config private/matter.yaml
```

The CLI executes only a fresh, one-shot, previously recorded dry-run trace. It
rejects stale traces older than 600 seconds and rejects trace IDs that already
completed real execution. Public example configs keep `execute_enabled: false`.

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
cargo test -p edgehome-cli execute_trace_posts_matter_bridge_from_private_config
```

The CLI integration test posts a fresh, previously recorded dry-run trace to a
local Matter bridge HTTP fixture using a private token file. It verifies the
harness boundary and request shape, not real Matter hardware behavior.

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
