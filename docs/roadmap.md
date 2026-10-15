# Roadmap

This roadmap describes engineering direction without turning future targets into
current claims.

## Current Baseline

The reusable SDK now consists of `edgehome-runtime` and `edgehome-inference`:
typed application tools, a host-driven bounded loop, configurable journals,
Ollama and OpenAI-compatible transports, six engine presets, and a non-home
document example. See the [SDK guide](edge-agent-sdk.md) and [engine validation
boundary](inference-engines.md). This is separate from the smart-home reference.

SDK priorities:

1. Build a reproducible model-task matrix: valid arguments, appropriate
   clarification, tool-result grounding, latency, and peak memory.
2. Verify llama.cpp on a physical CPU/ARM target before making edge performance
   claims; validate actual LM Studio and server versions beyond wire fixtures.
3. Add narrowly scoped application examples and a minimal Go/Python RPC entry
   point when the host protocol and cancellation semantics are specified.
4. Establish persistent journal and restart-reconciliation contracts before
   expanding to unattended, long-running tasks.

The following baseline describes the existing smart-home reference.

Implemented today:

- MiniCPM/MiniCPM5-class local 1B model profile.
- Backend-neutral candidate JSON contract.
- Rust parser, schema validation, normalization, registry resolution,
  capability checks, policy gates, and `GatedCommand`.
- Dry-run `ExecutionPlan`.
- Mock adapter.
- Home Assistant gateway boundary with dry-run payloads, opt-in REST execute,
  route validation, and optional post-state fetch.
- MQTT dry-run payload adapter and guarded publish executor.
- MIoT/Xiaomi bridge request adapter and opt-in bridge executor.
- Matter controller bridge request adapter and opt-in bridge executor.
- Local transport-level execute verification for Home Assistant, MQTT, MIoT
  bridge, and Matter bridge.
- Redacted and bounded executor response evidence for real backend paths.
- 108-case mock eval gate across 12 categories.
- Trace, replay, audit, storage, and low-memory profile documentation.

Not implemented today:

- Universal Xiaomi device support without a private MIoT bridge.
- Embedded full Matter controller and fabric commissioning.
- Production deployment hardening beyond documented gateway boundaries.
- Real-device execution enabled by default.
- Long-running physical 2GB ARM benchmark.

## Near-Term Work

1. Keep the public release gate stable in CI.
2. Add more negative eval cases for prompt injection, backend-access attempts,
   missing mappings, and unsupported capabilities.
3. Publish a separate real MiniCPM/Ollama eval report that does not overwrite
   mock gate metrics.
4. Add private real-device validation notes only after bridge/controller logs
   and hardware evidence exist.
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

1. Keep Home Assistant gateway execution gated and add more service/domain
   coverage only with tests.
2. Expand MQTT route/payload coverage only when concrete downstream schemas are
   configured.
3. Validate MIoT/Xiaomi through a private bridge and at least one real device
   before making device-support claims.
4. Validate Matter through a private controller bridge before making
   real-device control claims.

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
