# Release Checklist

This checklist keeps public releases aligned with implemented code and evidence.

## 1. Scope Review

Before every release, confirm these statements are still true:

- MiniCPM emits backend-neutral candidate JSON only.
- Rust validation, registry resolution, capability checks, and policy gates own
  command authority.
- Backend adapters translate verified internal plans.
- Real device execution is disabled by default.
- Unsupported backends fail closed.
- MQTT guarded publish remains opt-in and disabled by default.
- Real executor responses are sanitized before trace storage.
- Home Assistant and bridge base URLs reject query, fragment, and userinfo
  secret carriers.
- MIoT/Xiaomi and Matter are bridge request adapters. Do not claim universal
  device support without private bridge/controller validation evidence.

## 2. Required Commands

Run from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1
```

The script wraps the required commands, backend readiness checks, `git diff
--check`, and lightweight repository hygiene checks. To run the checks manually:

```powershell
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

The release gate must pass with:

```text
total_cases >= 100
category_count >= 12
pass_rate >= 1.0
schema_valid_rate >= 1.0
trace_coverage >= 1.0
false_allow_rate <= 0.0
fail_closed_rate >= 1.0
input_guard_flag_accuracy >= 1.0
```

## 3. Documentation Review

Check:

- README status table matches code.
- `CHANGELOG.md` includes the release.
- `docs/backend-adapter-contract.md` matches implemented adapters.
- `docs/bridge-api-contract.md` matches MIoT/Matter bridge executor behavior.
- `docs/schemas/*.schema.json` matches the current bridge request and response
  contract.
- `docs/mqtt-guarded-publish.md`, `docs/miot-bridge-adapter.md`,
  `docs/matter-bridge-adapter.md`, and `docs/home-assistant-gateway.md` match
  code behavior.
- CLI execute integration tests still cover fresh recorded dry-run traces for
  Mock, Home Assistant, MQTT, MIoT bridge, and Matter bridge.
- CLI execute stale-trace rejection still prevents backend calls for dry-run
  traces older than 600 seconds and records rejection evidence.
- `docs/customization.md` still says model JSON is canonical and backend
  mappings are configurable below the model.
- `docs/waic-one-page.md` avoids production, Xiaomi, Matter, MQTT, or 2GB
  hardware claims that are not supported by evidence.

## 4. Secrets Review

Before pushing:

```powershell
git status --short
git diff --cached
```

Confirm no committed file contains:

- Home Assistant tokens.
- MQTT broker credentials.
- MIoT, miIO, or MIoT bridge tokens.
- Matter bridge tokens, fabric details, controller secrets, node IDs, or
  endpoint IDs.
- Private token files or token-file paths that point into a real deployment.
- Private LAN URLs.
- Real device IDs that should not be public.
- Local SQLite demo databases.

Also confirm executor response evidence keeps only redacted or summarized backend
responses. Home Assistant post-state must not persist full `attributes`; MIoT
and Matter bridge responses must not persist private IDs, tokens, fabric data,
or private network URLs.

## 5. Release Notes

Each release note should include:

- Implemented changes.
- Eval gate summary.
- Known limitations.
- Backends implemented today.
- Backends that require private bridge/controller or real-device validation.

Do not mix mock eval metrics with real MiniCPM/Ollama metrics. If real-model
metrics are published, report model name, runtime, hardware, latency, retries,
fallbacks, and failure cases separately.
