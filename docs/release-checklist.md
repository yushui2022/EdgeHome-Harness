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
- MIoT/Xiaomi, Matter, and MQTT remain future targets unless code, config,
  golden tests, docs, and eval coverage prove otherwise.

## 2. Required Commands

Run from the repository root:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-release-gate.sqlite" eval cases\zh-home.yaml --gate
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
```

## 3. Documentation Review

Check:

- README status table matches code.
- `CHANGELOG.md` includes the release.
- `docs/backend-adapter-contract.md` matches implemented adapters.
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
- MIoT or miIO tokens.
- Private LAN URLs.
- Real device IDs that should not be public.
- Local SQLite demo databases.

## 5. Release Notes

Each release note should include:

- Implemented changes.
- Eval gate summary.
- Known limitations.
- Backends implemented today.
- Backends that remain future targets.

Do not mix mock eval metrics with real MiniCPM/Ollama metrics. If real-model
metrics are published, report model name, runtime, hardware, latency, retries,
fallbacks, and failure cases separately.
