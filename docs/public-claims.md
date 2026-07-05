# Public Claim Policy

This document keeps public EdgeHome Harness language aligned with implemented
code, tests, release evidence, and adapter boundaries.

## Required Positioning

Use this project-level positioning in public material:

```text
EdgeHome Harness is a Rust safety harness for MiniCPM-powered edge home command
agents. MiniCPM proposes backend-neutral candidate JSON, Rust validates and
gates the command, and backend adapters translate verified plans into
backend-specific dry-run or explicitly enabled execution payloads.
```

Short form:

```text
MiniCPM proposes. Rust decides. Adapters translate.
```

## Claims Allowed Today

These claims are supported by the current repository:

- The model-facing command contract is backend-neutral.
- Rust owns schema validation, normalization, device resolution, capability
  checks, policy gates, traceability, replay, and release evaluation.
- Public examples keep real-device execution disabled by default.
- The deterministic mock release gate covers at least 100 cases across at least
  12 categories.
- False allows and fail-open safety behavior are release-blocking failures.
- Home Assistant has a documented gateway boundary with dry-run service-call
  payloads, route validation, opt-in REST execution, and optional post-state
  readback.
- MQTT has dry-run route translation and guarded publish execution, disabled by
  default in public config.
- MIoT/Xiaomi support is a bridge request adapter plus opt-in bridge executor;
  real device support requires a private bridge and device-specific validation.
- Matter support is a controller-bridge request adapter plus opt-in bridge
  executor; real device control requires a private Matter controller bridge.
- Demo material can be backed by a git-ignored evidence bundle with a
  human-readable report, JSON artifacts, SHA-256 manifest, manifest schema, and
  standalone verifier.
- The reviewed real MiniCPM/Ollama eval result is model+harness evidence, not a
  standalone MiniCPM parsing benchmark.

## Claims Not Allowed Today

Do not claim or imply:

- broad production gateway readiness;
- universal Xiaomi, MIoT, or Matter device coverage;
- a full embedded Matter controller;
- a Mi Home or Home Assistant replacement;
- default-on real-device execution;
- model-generated vendor payloads or vendor-ready JSON;
- broad natural-language understanding from mock eval metrics;
- real Xiaomi or Matter hardware validation without a private evidence bundle;
- proven long-running operation on physical 2GB ARM hardware.

## Evidence Required To Upgrade Claims

Upgrade a public claim only when the matching evidence exists:

| Claim upgrade | Required evidence |
| --- | --- |
| Real Home Assistant deployment | Private config review, explicit execute trace, redacted request/response evidence, post-state readback, failure evidence, rollback notes. |
| Real MQTT deployment | Broker config review, route schema, publish trace, subscriber or broker log evidence, credential redaction, failure evidence. |
| Real MIoT/Xiaomi device support | Private bridge contract, device-specific route mapping, real device execution trace, state/readback or bridge result, secret redaction, rollback notes. |
| Real Matter control | Controller bridge contract, commissioning/controller boundary notes, node/endpoint mapping, real execution trace, state/readback or bridge result, secret redaction, rollback notes. |
| Physical 2GB hardware claim | Board model, OS image, model runtime, quantization details, memory/latency/fallback metrics, long-running stability log, reproducible commands. |
| Production gateway claim | Authentication model, authorization policy, observability, retry/backoff, operator controls, config migration, deployment guide, incident and rollback procedure, real-device evidence. |

## Release Check

`scripts\check-public-claims.ps1` scans release-facing documents for common
positive overclaims. `scripts\release-check.ps1` runs that lint before Rust
formatting, clippy, tests, eval, and backend readiness checks.

The lint is intentionally conservative. If a future document must quote a
normally blocked phrase for review purposes, keep that text out of public-facing
docs or add an inline `public-claim-allow:` note with a short reason.
