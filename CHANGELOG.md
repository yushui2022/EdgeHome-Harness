# Changelog

All notable project-facing changes should be recorded here.

This project follows a conservative release style: public claims must match
implemented code, tests, docs, and eval evidence.

## Unreleased

### Licensing

- Standardized the project license on MIT, with the full notice in `LICENSE`.
  Updated all workspace package metadata and public license references;
  third-party license and attribution notices remain in place.

### Added

- Resumable, bounded clarification with task-local IDs, free-text replies and
  host-supplied field resolution. Application preflight can allow, clarify or deny;
  execution repeats it and consumes an approved plan if conditions change.
- A real read-only filesystem application with bounded UTF-8 reads and traversal,
  path/link checks, RPC integration, and a frozen 20-task paired evaluation. Raw
  model traces require an explicit new output file; metadata logs remain separate.
- Opt-in action-first decisions, missing-value clarification, and bounded validation
  repair without automatic tool retries. Added system-role probes to detect model
  template/instruction problems that constant-only JSON checks miss.
- A 36-case multilingual reliability suite with a real length-conversion tool,
  repeated paired model runs, opt-in raw evidence, and independent Python rescoring.
  Wrong allowlisted proposals execute in test tools and remain scored as failures;
  evaluation no longer uses the answer key as an execution guard.
- `edgehome-kit`: independent application scaffolding, configuration/model
  diagnostics, tool-scoped run commands, sequential bounded stdio RPC, and a
  Python host example. Optional metadata journals use fresh files and byte caps.
- Task-scoped tool exposure enforced in prompts and host dispatch, lifecycle
  observers, usage counters, and cooperative cancellation in the generic runtime.
- Eight starter task cases across document and sensor fixtures, plus an external
  generated-application build and RPC smoke check. Model results remain separate.
- `edgehome-runtime`: a domain-independent agent SDK with typed tools, host-owned
  approval, bounded context and task loops, optional observation-before-answer
  checks, and a bounded metadata journal without HTTP or SQLite dependencies.
- `edgehome-inference`: bounded Ollama and OpenAI-compatible HTTP providers, with
  presets for Ollama, llama.cpp, LM Studio, vLLM, SGLang, and compatible services.
  Request deadlines, response-size caps, truncation/refusal errors, and local
  transport fixtures are included; presets are not device certification.
- A non-home document tool example, SDK integration and engine guides, and an
  actual model probe report separate from the existing smart-home evaluation.
- `edgehome-agent`: an embeddable, bounded single-action runtime with host-owned
  budgets, confirmation, private expiring plans, instance-local execution state,
  and explicit unknown-outcome handling without automatic execution retries.
- Hardware-free agent example, fault-injection boundary tests, and a Chinese
  reference guide for adapting the pattern to constrained-device applications.
- Shared execution transaction rejects unsupported time-conditioned dispatch
  before contacting a backend; dry-run slot extraction remains available.
- Traced execution uses trace identity for idempotency, allowing separately
  approved repeated actions after cooldown; dispatched plan copies are marked
  as execution rather than dry-run.

- GitHub Actions CI for formatting, Clippy, workspace tests, and the release
  eval gate.
- Contributor, security, roadmap, release checklist, and GitHub issue/PR
  templates.
- MQTT dry-run payload adapter with golden payload and fail-closed topic tests.
- Home Assistant golden payload documentation.
- Real MiniCPM/Ollama eval report workflow and script.
- Guarded MQTT publish executor, disabled by default.
- MIoT/Xiaomi bridge request adapter and opt-in bridge executor.
- Matter controller bridge request adapter and opt-in bridge executor.
- Home Assistant gateway route validation and optional post-state fetch after
  explicit execution.
- Redacted executor response evidence for Home Assistant, MIoT bridge, and
  Matter bridge execution paths.
- Bridge API contract documentation for private MIoT/Matter bridge
  implementations.
- Parsed bridge base URL validation shared by executor and backend readiness
  checks.
- Home Assistant base URL validation now rejects query strings, fragments,
  username/password userinfo, non-HTTP schemes, and missing hosts during
  readiness checks and execution.
- Local release check script for formatting, Clippy, workspace tests, eval gate,
  backend readiness checks, diff hygiene, and lightweight repository hygiene.
- Machine-readable bridge JSON schemas for MIoT request, Matter request, and
  shared bridge response contracts.
- Local HTTP bridge poster tests for bearer-authenticated JSON requests and
  redacted non-success responses.
- Local HTTP Home Assistant gateway tests for opt-in service execution,
  post-state fetch, token isolation, and sanitized non-success response bodies.
- Local MQTT broker fixture test for the default `rumqttc` publish path.
- MIoT and Matter bridge schema drift tests against serialized Rust request
  shapes.
- Bridge token-file support for MIoT/Matter private bridge configs.
- CLI execute integration tests for recorded dry-run traces across Mock, Home
  Assistant, MQTT, MIoT bridge, and Matter bridge paths.
- CLI execute freshness guard rejects dry-run traces older than 600 seconds and
  records stale-trace rejection evidence before any backend call.
- CLI execute one-shot guard rejects a second real execution attempt for a trace
  after `real_execution_completed` has already been recorded.
- CLI execute now records backend/config/transport failures as redacted
  executor failure evidence, a failed step, and an audit event without marking
  the trace as completed.
- Rewrote the public demo walkthrough, Home Assistant demo guide, and docs
  index in English with clearer claim boundaries and verification steps.
- Demo script can now generate a local public demo evidence bundle with a
  Markdown summary and per-step JSON artifacts under ignored `artifacts/`.
- Demo evidence bundles now include a SHA-256 manifest with git commit, tracked
  dirty flag, artifact sizes, hashes, model mode, execution boundary, and public
  claim boundary.
- Standalone demo evidence verifier and manifest JSON Schema for reviewing
  generated public demo bundles.
- Public claim policy and release-check lint for catching unsupported
  production, Xiaomi/MIoT, Matter, default execution, and vendor-ready JSON
  claims in release-facing documents.
- Public release baseline document for GitHub releases, demos, WAIC one-page
  material, evidence bundles, and non-claim boundaries.

## 0.1.0 - 2026-07-04

### Added

- Rust workspace for the EdgeHome Harness prototype.
- Backend-neutral MiniCPM candidate JSON boundary.
- Parser, schema validation, semantic normalization, registry resolution,
  capability checks, policy gates, and typed `GatedCommand` boundary.
- Dry-run `ExecutionPlan` generation.
- Mock adapter and Home Assistant demo payload adapter.
- SQLite-backed evidence, trace, replay, audit, and memory support.
- Low-memory MiniCPM5-1B oriented runtime profile.
- 108-case mock release eval gate across 12 categories.
- Public README, WAIC one-page, architecture, customization, adapter, deployment,
  and low-memory documentation.

### Not Included

- Universal smart-home gateway support.
- Universal MIoT/Xiaomi or Matter device support.
- Real Xiaomi or Matter hardware validation evidence.
- Real-device execution enabled by default.
- Long-running benchmark on a physical 2GB ARM board.
