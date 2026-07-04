# MQTT Guarded Publish

MQTT support has two layers:

```text
Dry-run adapter:
  verified ExecutionPlan -> configured topic + JSON payload

Guarded executor:
  verified ExecutionPlan -> rumqttc publish
```

Real publish is disabled by default. Dry-run does not require a broker, username,
password, or network access.

## Boundary

MiniCPM must not emit MQTT topics. The topic comes from the trusted device
registry:

```yaml
devices:
  - device_id: hallway_light
    backend: mqtt
    backend_entity_id: home/hallway/light/set
```

The executor profile owns broker connection settings:

```yaml
broker_url_env: EDGEHOME_MQTT_BROKER_URL
username_env: EDGEHOME_MQTT_USERNAME
password_env: EDGEHOME_MQTT_PASSWORD
qos: 0
retain: false
execute_enabled: false
```

Read-only readiness check:

```powershell
cargo run -q -p edgehome-cli -- backend check --backend mqtt --registry configs/devices.mqtt.example.yaml
```

The check validates topic routes, adapter config, QoS, execution switch, and
broker secret availability. It does not connect to the broker or publish a
message.

## Dry-Run Payload

Example internal command:

```json
{
  "device_id": "hallway_light",
  "action": "turn_on"
}
```

Expected MQTT dry-run payload:

```json
{
  "backend": "mqtt",
  "device_id": "hallway_light",
  "topic": "home/hallway/light/set",
  "qos": 0,
  "retain": false,
  "action": "turn_on",
  "payload": {
    "power": "on"
  }
}
```

## Real Publish

Real publish requires all of:

```text
execute_enabled = true
EDGEHOME_MQTT_BROKER_URL set, or broker_url configured privately
dry-run plan generated
gate accepted
policy not denied
user confirmation when required by risk
rate limit and idempotency checks passed
```

Example private local test:

```powershell
$env:EDGEHOME_MQTT_BROKER_URL = "mqtt://127.0.0.1:1883"
$env:EDGEHOME_MQTT_USERNAME = "edgehome"
$env:EDGEHOME_MQTT_PASSWORD = "local-test-password"
```

Do not commit private broker URLs or credentials.

CLI shape:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite execute <trace_id> --confirm --backend-config private/mqtt.yaml
```

The CLI executes only a previously recorded dry-run trace. Public example
configs keep `execute_enabled: false`.

## Failure Behavior

The MQTT adapter fails closed for:

```text
missing topic route
invalid topic containing wildcards
missing broker URL during real execute
invalid broker URL
invalid QoS
publish timeout or broker error
execute_enabled = false
policy denied
```

## Tests

Relevant tests:

```powershell
cargo test -p edgehome-executor mqtt
cargo test -p edgehome-executor dry_run_planner_translates_mqtt
```

The MQTT test group includes a local broker fixture that verifies the default
`RumqttcMqttPublisher` path performs an MQTT CONNECT and sends a PUBLISH packet
to the configured topic with the expected JSON payload. This is still a local
transport test, not evidence that any particular home platform accepts a
universal MQTT smart-home schema.

## Claim Boundary

MQTT is a transport, not a universal smart-home schema. EdgeHome Harness
provides a deterministic topic/payload adapter and guarded publish path. It does
not claim that one MQTT JSON format works for every home platform.
