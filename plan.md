# EdgeHome Harness Commercial-Grade Completion Plan

Last updated: 2026-07-04

This file is the handoff plan for future Codex/context-compressed sessions. Read
it before continuing work.

Core positioning remains unchanged:

```text
MiniCPM proposes.
Rust decides.
Adapters translate.
```

The project must not become "let the small model output vendor JSON". MiniCPM
outputs one fixed, backend-neutral candidate JSON. Rust validates and gates it.
Adapters translate verified internal plans into backend payloads or bridge
requests.

## 1. Current Implemented State

Implemented code paths:

```text
MiniCPM/Mock candidate JSON
  -> Rust schema validation / normalization
  -> DeviceRegistry / DeviceResolver
  -> GateEngine::verify
  -> GatedCommand
  -> DryRunPlanner::plan_gated
  -> BackendAdapter payload
  -> trace / replay / eval gate
```

Current backend status:

| Backend | Current state | Public claim allowed |
| --- | --- | --- |
| Mock | Implemented | Deterministic dry-run and regression eval baseline |
| Home Assistant | Gateway boundary implemented and locally verified | Dry-run service payloads, opt-in REST execution, route validation, optional post-state fetch, executor tests, and CLI execute test against local HTTP fixture |
| MQTT | Dry-run and guarded publish implemented and locally verified | Configured topic/payload dry-run, opt-in broker publish, executor and CLI local MQTT broker CONNECT/PUBLISH tests |
| MIoT / Xiaomi | Bridge request adapter implemented and locally verified | Verified commands can become MIoT bridge requests; CLI execute test posts to local bridge fixture; real Xiaomi support still requires private bridge/device validation |
| Matter | Bridge request adapter implemented and locally verified | Verified commands can become Matter controller bridge requests; CLI execute test posts to local bridge fixture; real Matter control still requires private bridge/controller validation |
| Bridge API contract | Implemented | Documents private MIoT/Matter bridge HTTP endpoints, response/error semantics, and URL rules |
| Bridge JSON schemas | Implemented and drift-tested | Machine-readable MIoT request, Matter request, and bridge response schemas checked against serialized Rust requests |
| Backend check CLI | Implemented | Read-only route/config readiness validation for Home Assistant, MQTT, MIoT bridge, and Matter bridge |
| Execution evidence privacy | Implemented | Real backend responses are redacted/bounded before trace storage |
| Backend URL validation | Implemented | HA and bridge URLs reject query, fragment, userinfo, non-HTTP schemes, and missing hosts |
| Bridge token file support | Implemented | MIoT/Matter private bridge tokens can come from env vars or private token files outside the repository |
| Local release check script | Implemented | Runs fmt, clippy, tests, eval gate, backend checks, diff check, and repository hygiene scan |

Important implemented files:

```text
crates/edgehome-cli/src/main.rs
crates/edgehome-executor/src/mqtt.rs
crates/edgehome-executor/src/miot.rs
crates/edgehome-executor/src/matter.rs
crates/edgehome-executor/src/home_assistant.rs
crates/edgehome-executor/src/bridge.rs
crates/edgehome-registry/src/lib.rs
configs/adapters/mqtt.example.yaml
configs/adapters/miot.example.yaml
configs/adapters/matter.example.yaml
configs/devices.mqtt.example.yaml
configs/devices.miot.example.yaml
configs/devices.matter.example.yaml
docs/mqtt-guarded-publish.md
docs/miot-bridge-adapter.md
docs/matter-bridge-adapter.md
docs/bridge-api-contract.md
docs/schemas/miot-bridge-request.schema.json
docs/schemas/matter-bridge-request.schema.json
docs/schemas/bridge-response.schema.json
docs/home-assistant-gateway.md
docs/home-assistant-golden-payloads.md
docs/real-minicpm-eval-report.md
scripts/run-real-minicpm-eval.ps1
scripts/release-check.ps1
```

## 2. Non-Negotiable Safety Rule

Do not implement real device execution as default-on.

Commercial-grade execution means:

```text
default dry-run
explicit opt-in execute_enabled
secret isolation
gate accepted
policy not denied
risk-aware confirmation
rate limit
idempotency
audit trail
redacted executor evidence
post-state verification where available
clear failure modes
```

Default-on real device execution would be less professional, not more
commercial.

## 3. What Is Still Not Proven

Do not overclaim these until evidence exists:

```text
Universal Xiaomi / MIoT support
Embedded full Matter controller
Production replacement for Home Assistant
Default real-device execution
Real Xiaomi device validation
Real Matter controller/device validation
Long-running physical 2GB ARM validation
Broad natural-language benchmark quality
```

The correct public language is:

```text
Bridge request adapter implemented.
Real device support requires configured private bridge/controller and
device-specific validation evidence.
```

## 4. Recovery Checklist For Any Future Session

From repo root:

```powershell
cd C:\Users\xiaoy\Desktop\edge-home\EdgeHome-Harness
git status --short
git log -5 --oneline
git diff --stat
```

If there are uncommitted changes, inspect them before editing:

```powershell
git diff
```

Never revert user changes unless the user explicitly asks.

## 5. Required Verification Before Commit

Run these before a release-quality commit:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-release-gate.sqlite" eval cases\zh-home.yaml --gate
cargo run -q -p edgehome-cli -- backend check --backend home_assistant --registry configs\devices.home_assistant.example.yaml
cargo run -q -p edgehome-cli -- backend check --backend mqtt --registry configs\devices.mqtt.example.yaml
cargo run -q -p edgehome-cli -- backend check --backend miot --registry configs\devices.miot.example.yaml
cargo run -q -p edgehome-cli -- backend check --backend matter --registry configs\devices.matter.example.yaml
git diff --check
```

Minimum eval gate:

```text
total_cases >= 100
category_count >= 12
pass_rate = 1.0
schema_valid_rate = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
```

## 6. Commit Cadence

Commit and push after each coherent milestone:

```text
1. executor/backend code + tests
2. docs/config alignment
3. eval report workflow
4. final release wording and checklist
```

Suggested commit messages:

```text
executor: add guarded backend adapters
docs: align backend adapter boundaries
eval: add real MiniCPM report workflow
docs: update release scope and checklist
```

## 7. Remaining Work To Reach Strong Public Release

### M1: Finish Local Verification

Goal: all current code and docs pass CI-quality checks.

Required:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
mock eval gate passes
backend check passes for HA/MQTT/MIoT/Matter example registries
git diff --check
README/backend docs agree with code
```

Exit criteria:

```text
No clippy warnings
No failing tests
No stale "future target" language for implemented bridge adapters
No claim that real execution is default-on
ExecutorResponse evidence is redacted/summarized before trace storage
CLI execute tests cover Mock, Home Assistant, MQTT, MIoT bridge, and Matter bridge
No committed secrets
```

### M2: Real MiniCPM/Ollama Eval Report

Goal: separate real-model behavior from deterministic mock release gate.

Run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run-real-minicpm-eval.ps1 -ModelName openbmb/minicpm5:latest -TimeoutMs 60000 -NumPredict 128
```

The script writes raw output to `artifacts/`, which is ignored by git.

Publish only reviewed summary metrics in:

```text
docs/real-minicpm-eval-report.md
```

Latest reviewed run:

```text
date = 2026-07-04 21:29:47 +08:00
model = openbmb/minicpm5:latest
model_id = 08239e8f70e0
cases = 108
passed = 104
failed = 4
pass_rate = 0.9630
schema_valid_rate = 0.9500
trace_coverage = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
fallback_rate = 0.7778
deterministic_repair_or_fallback_count = 84
latency_avg_ms = 4829.49
latency_p95_ms = 6023
```

Interpretation: this is a strong model+harness path result, but not a pure
MiniCPM parsing result. The high pass rate depends on deterministic Rust-side
candidate repair/fallback in 84 of 108 cases. Do not market it as standalone
model production accuracy.

Required report fields:

```text
model name
ollama version
hardware
OS
case count
pass_rate
schema_valid_rate
false_allow_rate
fail_closed_rate
latency avg / p95
top failure categories
```

Do not present mock gate metrics as real-model metrics.

### M3: Optional Real Integration Evidence

Goal: strengthen public credibility without overclaiming.

MQTT:

```text
Use a local test broker.
Set EDGEHOME_MQTT_BROKER_URL privately.
Run an opt-in integration script or ignored test.
Record only redacted evidence.
```

MIoT/Xiaomi:

```text
Use a private MIoT bridge.
Bridge owns did/siid/piid/aiid/token.
Validate at least one real device before claiming device support.
Record route ID, action, redacted response, and post-state evidence.
```

Matter:

```text
Use a private Matter controller bridge.
Bridge owns fabric/node/endpoint/cluster mapping.
Validate at least one real device before claiming real Matter control.
Record redacted controller response and state evidence.
```

### M4: Final Public Release Wording

Allowed:

```text
Rust safety harness for MiniCPM-powered edge home command agents.
Backend-neutral model candidate JSON.
Rust validation, registry resolution, policy gates, dry-run planning, trace, eval.
Mock, Home Assistant, MQTT, MIoT bridge, and Matter bridge adapter boundaries.
Real execution is explicit opt-in and disabled by default.
```

Not allowed:

```text
Production smart-home replacement.
Universal Xiaomi support.
Embedded full Matter controller.
Default real-device execution.
Model outputs vendor-ready JSON.
Mock eval proves broad language understanding.
```

## 8. Final Target

The final public project should read as:

```text
EdgeHome Harness is a reproducible Rust safety harness for MiniCPM-class local
smart-home command agents. It keeps model output backend-neutral, validates and
gates commands in Rust, translates verified plans through backend adapters, and
keeps real device execution explicit, auditable, and disabled by default.
```

The strongest defensible claim is not "we support every smart-home ecosystem".
It is:

```text
We provide a safe, typed, testable boundary between a small local model and
backend-specific smart-home execution adapters.
```
