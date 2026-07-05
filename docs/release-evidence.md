# Release Evidence

This document defines the evidence bundle used to support public EdgeHome
Harness claims. It keeps demo language tied to reproducible commands instead of
unchecked README statements.

## Evidence Boundary

The public release evidence proves:

- The model-facing JSON contract is backend-neutral.
- Rust validation, normalization, registry resolution, policy gates, and dry-run
  planning run before any adapter payload exists.
- The deterministic mock release gate covers at least 100 cases across at least
  12 categories.
- False allows are treated as release-blocking failures.
- Dangerous and unsupported commands fail closed.
- Trace, replay, memory, backend readiness, and output-governor evidence can be
  generated from a clean checkout.
- Home Assistant, MQTT, MIoT bridge, and Matter bridge configs can be validated
  without contacting real devices.
- Real device execution remains disabled by default.

The public release evidence does not prove:

- Universal smart-home support.
- Universal Xiaomi / MIoT device support.
- A full Matter controller implementation.
- Real Xiaomi hardware validation.
- Real Matter hardware validation.
- Default-on real-device execution.
- Standalone MiniCPM parsing accuracy independent of harness repair, validation,
  fallback, and policy gates.

## Required Release Check

Run this before a public release:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1
```

The command runs repository hygiene checks, JSON schema syntax checks, formatting,
clippy with `-D warnings`, workspace tests, the 108-case release eval gate,
backend readiness checks, and `git diff --check`.

## Optional Demo Evidence Smoke

Run this when preparing public demo material, a WAIC one-page, or a release
announcement:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1 -WithDemoSmoke
```

This includes the required release check and then generates a public demo smoke
bundle under:

```text
artifacts\release-demo-smoke
```

The `artifacts/` directory is ignored by git. Do not commit generated demo
artifacts unless a future release process explicitly creates a reviewed,
redacted, versioned evidence snapshot.

To choose explicit paths:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1 `
  -WithDemoSmoke `
  -DemoDatabasePath "$env:TEMP\edgehome-release-demo.sqlite" `
  -DemoOutputDir artifacts\release-demo-smoke
```

## Demo Bundle Contents

The demo smoke bundle currently contains:

| File | Purpose |
| --- | --- |
| `public-demo-report.md` | Human-readable summary and claim boundary. |
| `01-release-gate.json` | Eval gate metrics and release checks. |
| `02-ordinary-dry-run.json` | Normal command dry-run evidence. |
| `03-slot-dry-run.json` | Slot extraction dry-run evidence. |
| `04-replay-summary.json` | Replay summary for an existing trace. |
| `05-trace-frame.json` | Exported trace frame evidence. |
| `06-short-memory.json` | Short-memory relative command evidence. |
| `07-long-memory.json` | Confirmed long-memory alias evidence. |
| `08-dangerous-blocked.json` | Fail-closed dangerous-command evidence. |
| `09-low-memory-pressure.json` | Runtime pressure fallback decisions. |
| `10-backend-readiness.json` | Read-only adapter readiness checks. |
| `11-output-governor-test.txt` | Focused output-governor test output. |

## Current Gate Thresholds

The release gate must pass with:

```text
total_cases >= 100
category_count >= 12
pass_rate >= 1.0
schema_valid_rate >= 1.0
trace_coverage >= 1.0
intent_accuracy >= 0.95
slot_accuracy >= 0.9
input_guard_flag_accuracy >= 1.0
false_allow_rate <= 0.0
fail_closed_rate >= 1.0
retry_rate <= 0.3
```

## How To Use In Public Material

Safe claim:

```text
EdgeHome Harness implements a backend-neutral MiniCPM command pipeline with Rust
validation, gated dry-run planning, trace/audit evidence, and guarded backend
adapter boundaries for Mock, Home Assistant, MQTT, MIoT bridge, and Matter
bridge.
```

Unsafe claim:

```text
EdgeHome Harness fully supports Xiaomi and Matter devices in production.
```

Use the unsafe claim only after a separate private-device evidence bundle proves
real bridge/controller execution, state readback, failure handling, redaction,
and operator-controlled rollback for those devices.
