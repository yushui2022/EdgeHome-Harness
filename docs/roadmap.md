# Roadmap

This roadmap describes engineering direction without turning future targets into
current claims.

## Current Baseline

Implemented today:

- MiniCPM/MiniCPM5-class local 1B model profile.
- Backend-neutral candidate JSON contract.
- Rust parser, schema validation, normalization, registry resolution,
  capability checks, policy gates, and `GatedCommand`.
- Dry-run `ExecutionPlan`.
- Mock adapter.
- Home Assistant demo payload adapter.
- 108-case mock eval gate across 12 categories.
- Trace, replay, audit, storage, and low-memory profile documentation.

Not implemented today:

- Real MIoT/Xiaomi adapter.
- Matter controller adapter.
- MQTT adapter.
- Production smart-home gateway behavior.
- Real-device execution enabled by default.
- Long-running physical 2GB ARM benchmark.

## Near-Term Work

1. Keep the public release gate stable in CI.
2. Add more negative eval cases for prompt injection, backend-access attempts,
   missing mappings, and unsupported capabilities.
3. Publish a separate real MiniCPM/Ollama eval report that does not overwrite
   mock gate metrics.
4. Harden Home Assistant demo documentation and examples around dry-run,
   secrets, and disabled-by-default execution.
5. Improve trace and replay examples for failure diagnosis.

## Adapter Work

Backend adapters should be added only when they have:

- A documented adapter contract.
- Config-driven backend mappings.
- No model-generated backend IDs or vendor payloads.
- Golden dry-run payload tests.
- Fail-closed tests for missing routes and unsupported commands.
- Documentation updates that clearly distinguish implemented support from future
  targets.

Candidate order:

1. Expand Home Assistant demo coverage while keeping real execution gated.
2. Add MQTT as an app-defined topic/payload adapter, not as a universal
   smart-home standard.
3. Research MIoT/Xiaomi mappings with real device profiles before claiming
   support.
4. Treat Matter as a controller integration project, not a JSON payload format.

## Hardware Work

The low-memory profile is a design constraint today. A stronger hardware claim
requires:

- Physical ARM board details.
- Model runtime and quantization details.
- Memory, latency, retry, and fallback metrics.
- Long-running stability results.
- Reproducible scripts and logs.

Until then, use "2GB-4GB design target" or "QEMU/low-memory pre-validation"
instead of "proven on 2GB hardware".

## Non-Goals

- Making MiniCPM emit vendor-ready JSON.
- Letting prompt text define risk policy.
- Silently falling back to mock when an adapter is unsupported.
- Replacing Mi Home, Home Assistant, Matter, MQTT, or a smart speaker.
- Treating mock eval pass rates as broad natural-language understanding.
