# EdgeHome Harness

## A Rust Safety Harness For MiniCPM-Powered Edge Home Command Agents

```text
MiniCPM proposes. Rust decides. Adapters translate.
```

EdgeHome Harness explores a narrow but practical edge-AI problem: how to let a
small local MiniCPM-class model participate in smart-home command generation
without giving the model direct authority over devices, backend IDs, tokens, or
execution.

The project is not a smart speaker replacement, not a Home Assistant
replacement, and not a production smart-home gateway. It is a safety harness
and evaluation prototype for constrained local command agents.

## Why It Exists

Small local models can be useful for short Chinese smart-home slot extraction,
but they can also repeat, drift, omit fields, produce malformed JSON, or make
unsafe assumptions.

EdgeHome Harness keeps the model role deliberately small:

```text
The model emits backend-neutral candidate JSON.
The Rust harness validates, normalizes, resolves, gates, traces, and evaluates.
Backend adapters translate verified plans into backend-specific payloads.
```

This makes model output auditable and prevents a small model from inventing
vendor-specific identifiers such as Home Assistant `entity_id`, MIoT `did`,
Matter cluster IDs, MQTT topics, local URLs, or tokens.

## Command Pipeline

```text
Chinese user command
  -> Input Guard
  -> MiniCPM / Mock candidate JSON
  -> Output Governor
  -> Rust schema validation
  -> Semantic normalization
  -> DeviceRegistry / DeviceResolver
  -> Capability and policy gates
  -> GatedCommand
  -> ExecutionPlan
  -> BackendAdapter payload
  -> Mock / Home Assistant / MQTT / MIoT bridge / Matter bridge payload
  -> Trace / Replay / Eval gate
```

Layer 1: Model candidate JSON

The MiniCPM output is untrusted. It is short, backend-neutral, and owned by this
project's fixed schema.

Layer 2: Verified internal command

Rust owns schema validation, slot normalization, registry-based device
resolution, capability checks, policy decisions, memory boundaries, and the
typed `GatedCommand` handoff.

Layer 3: Backend adapter payload

Adapters translate verified `ExecutionPlan` values into backend-specific
dry-run payloads. Vendor-specific routes live here, not in the model prompt.

## Current Evidence

Current evidence is scoped to deterministic mock evaluation, real MiniCPM
model-path evaluation, dry-run planning, traceability, and adapter boundaries:

- 108 mock eval cases across 12 categories.
- `pass_rate = 1.0`.
- `schema_valid_rate = 1.0`.
- `trace_coverage = 1.0`.
- `false_allow_rate = 0.0`.
- `fail_closed_rate = 1.0`.
- Typed `GatedCommand` boundary before dry-run planning.
- Golden tests for exact Mock, Home Assistant, MQTT, MIoT bridge, and Matter
  bridge adapter payloads.
- Missing Home Assistant routes and invalid entity IDs fail closed.
- MQTT guarded publish, MIoT bridge execution, and Matter bridge execution are
  implemented as opt-in paths and disabled by default.
- Real backend executor responses are redacted and bounded before trace storage.
- Latest reviewed real MiniCPM/Ollama run: 108 cases, 104 passed end-to-end,
  `false_allow_rate = 0.0`, `fail_closed_rate = 1.0`; 84 cases used
  deterministic repair/fallback, so this is model+harness evidence rather than
  standalone model accuracy.

This verifies covered harness regressions, not broad natural-language
understanding, production readiness, or real-device deployment at scale.

## Current Scope

Implemented:

- MiniCPM5-1B oriented low-memory runtime profile.
- Mock model path for deterministic demos and eval.
- Ollama structured-output adapter.
- Output governor for overlong, malformed, repetitive, or unstable output.
- Device registry and registry-based device resolution.
- Policy and capability gates with fail-closed behavior.
- `GatedCommand` and dry-run `ExecutionPlan` boundary.
- Mock backend adapter.
- Home Assistant gateway boundary.
- MQTT guarded publish adapter.
- MIoT/Xiaomi bridge request adapter.
- Matter controller bridge request adapter.
- SQLite-backed trace, replay, audit, evidence, and memory.
- Release-gated eval suite.

Not claimed:

- Production-ready smart-home gateway.
- Universal Xiaomi / MIoT / Matter support, or MQTT real broker publish by
  default.
- Real-device execution enabled by default.
- Full Home Assistant production coverage.
- Model-generated vendor-ready JSON.
- Long-running benchmark on a real 2GB ARM board.

## Customization Model

The model output contract stays fixed and safe.

Customization happens below the model:

- Add devices and aliases in the device registry.
- Define supported capabilities and value ranges.
- Configure backend routes such as Home Assistant entity IDs, MQTT topics, MIoT
  bridge route IDs, and Matter bridge route IDs.
- Add a new backend adapter with golden payload tests and fail-closed tests.

This is the core design choice:

```text
Do not ask the small model to emit each vendor's JSON.
Keep model JSON canonical.
Customize the registry and adapter mappings.
```

## Backend Roadmap

Current:

- Mock: implemented for deterministic dry-run and eval.
- Home Assistant: gateway boundary implemented for service-call payloads,
  opt-in REST execution, route validation, and optional post-state fetch.
- MQTT: dry-run payload adapter and guarded publish executor implemented; real
  broker publish remains disabled by default.
- MIoT / Xiaomi: bridge request adapter implemented; real Xiaomi device support
  requires a private bridge and device-specific validation.
- Matter: bridge request adapter implemented; real control requires a private
  Matter controller bridge.

Future validation targets:

- MQTT publish evidence against a local broker.
- MIoT bridge logs with at least one real Xiaomi device.
- Matter controller bridge logs with at least one real Matter device.

Unsupported backends must fail closed until code, configuration, golden tests,
secret-handling tests, and documentation all exist.

## Public Positioning

EdgeHome Harness demonstrates how a MiniCPM-class local model can safely produce
backend-neutral smart-home command candidates, while Rust owns validation,
device resolution, policy gating, traceability, evaluation, and backend payload
translation.

The value is not that a 1B model can control everything. The value is that a
small model can be given a narrow candidate-generation role inside a strong,
typed, replayable, and fail-closed harness.
