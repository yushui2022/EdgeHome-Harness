# Customization Contract

EdgeHome Harness is customizable at the registry and adapter layers. The model
output schema stays fixed.

This is the key design rule:

```text
Model output is not customized.
Device registry and backend mappings are customized.
```

## Why The Model Schema Stays Fixed

Different smart-home backends do not share one JSON format.

Examples:

```text
Home Assistant uses service-call payloads and entity IDs.
MIoT uses device/spec identifiers such as did, siid, piid, and aiid.
Matter uses node, endpoint, cluster, attribute, and command concepts.
MQTT only defines publish/subscribe transport; topic and payload are app-defined.
```

Asking MiniCPM to emit those vendor payloads directly would weaken the harness:

```text
It could hallucinate vendor IDs.
It would mix private backend routing into model context.
It would make schema validation vendor-specific and brittle.
It would reduce the value of Rust-side safety checks.
```

Therefore MiniCPM emits only `ModelCandidate`.

## What Users Can Customize

Users can customize:

```text
device_id
aliases
room
device_type
risk_level
capability rules
backend kind
backend_entity_id
adapter route mappings
```

Users should not customize MiniCPM to emit:

```text
Home Assistant entity_id
MIoT did / siid / piid / aiid
Matter node / endpoint / cluster
MQTT topic
tokens
backend URLs
vendor API payloads
```

## Adding A Device Of An Existing Type

If the room, device type, action, and backend kind already exist in the Rust
types, adding a device is mainly a registry change.

Example:

```yaml
devices:
  - device_id: living_room_air_conditioner
    aliases: ["living room ac", "main ac"]
    room: living_room
    device_type: air_conditioner
    backend: home_assistant
    backend_entity_id: climate.living_room_ac
    risk_level: medium
```

The capability catalog must also allow the requested action:

```yaml
capabilities:
  air_conditioner:
    - action: turn_on
    - action: turn_off
    - action: set_temperature
      min: 16
      max: 30
      unit: celsius
    - action: set_mode
```

After editing the registry, run:

```powershell
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-custom-device.sqlite" eval cases\zh-home.yaml --gate
```

## Current Limits

The current project still represents these as Rust enums:

```text
Room
DeviceType
Action
BackendKind
```

That means:

```text
Adding another light, air conditioner, camera, lock, or similar supported device can mostly be config-driven.
Adding a new room enum value requires code changes.
Adding a new device category requires code changes.
Adding a new action requires code changes.
Adding a new backend adapter requires code and tests.
```

This is intentional for the current release hardening phase. It keeps the safety
contract simple and easier to audit.

## Adding A New Device Category

Example: adding a humidifier.

Recommended short-term workflow:

```text
1. Add Humidifier to DeviceType.
2. Add any missing actions, such as SetHumidity, to Action.
3. Extend schema tests and serialization tests.
4. Add capability rules in devices.yaml.
5. Extend parser or prompt examples if needed.
6. Add eval cases for normal, unsupported, and unsafe paths.
7. Add adapter payload tests for any real backend mapping.
```

Do not bypass this by asking the model to emit arbitrary `device_type` strings.

## Adding A New Backend Mapping

Backend mapping belongs in adapter code and adapter profile configuration.

The workflow should be:

```text
1. Define or extend a BackendAdapter.
2. Define the adapter profile shape.
3. Add example config without secrets.
4. Add golden tests for payload generation.
5. Add fail-closed tests for missing or unsupported mappings.
6. Update the README support matrix only after tests pass.
```

## Home Assistant Example

For Home Assistant, `backend_entity_id` is an entity ID:

```yaml
devices:
  - device_id: hallway_light
    aliases: ["hallway light"]
    room: hallway
    device_type: light
    backend: home_assistant
    backend_entity_id: light.hallway
    risk_level: low
```

The adapter translates the verified plan into a service call:

```json
{
  "backend": "home_assistant",
  "entity_id": "light.hallway",
  "service": "light.turn_on",
  "service_path": "/api/services/light/turn_on"
}
```

The model never emits `light.hallway`.

## MQTT Example

MQTT dry-run payload translation and guarded publish are implemented. Real
broker publish remains disabled unless explicit private execution config is
enabled. MQTT route mappings live in the device registry:

```yaml
device_id: hallway_light
backend: mqtt
backend_entity_id: home/hallway/light/set
```

Topics are registry-owned. MiniCPM must not emit MQTT topics.

## MIoT Bridge Example

MIoT/Xiaomi uses a bridge request adapter. Real Xiaomi device support requires a
private MIoT bridge that owns device/spec IDs and tokens:

```yaml
device_id: bedroom_air_conditioner
backend: miio_local
backend_entity_id: miot.bedroom_ac
```

The bridge maps `miot.bedroom_ac` to real `did / siid / piid / aiid` values.
MiniCPM must not emit those values.

## Matter Bridge Example

Matter uses a controller bridge request adapter. The bridge owns fabric, node,
endpoint, cluster, command, and attribute mapping:

```yaml
device_id: hallway_light
backend: matter_bridge
backend_entity_id: matter.hallway_light
```

The bridge maps `matter.hallway_light` to the actual Matter controller route.
MiniCPM must not emit Matter node or cluster IDs.

## Acceptance Rule

A customization is acceptable only if this remains true:

```text
MiniCPM emits backend-neutral candidate JSON.
Rust validates and resolves the command.
The adapter generates backend-specific payload.
Unsupported or missing mappings fail closed.
```
