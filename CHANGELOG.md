# Changelog

All notable project-facing changes should be recorded here.

This project follows a conservative release style: public claims must match
implemented code, tests, docs, and eval evidence.

## Unreleased

### Added

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

- Production smart-home gateway support.
- Real MIoT/Xiaomi, Matter, or MQTT adapters.
- Real-device execution enabled by default.
- Long-running benchmark on a physical 2GB ARM board.
