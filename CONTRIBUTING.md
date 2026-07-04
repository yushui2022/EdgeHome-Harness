# Contributing

Thank you for improving EdgeHome Harness. This project is intentionally narrow:
a small local model proposes backend-neutral smart-home command candidates, and
Rust decides what is valid, safe, traceable, and translatable.

## Non-Negotiable Boundaries

Every contribution must preserve these boundaries:

- `ModelOutput != Command`.
- MiniCPM output remains backend-neutral candidate JSON.
- The model must not emit real `device_id`, Home Assistant `entity_id`, MIoT
  `did / siid / piid / aiid`, Matter route IDs, MQTT topics, backend URLs,
  tokens, or execution switches.
- Device identity, capabilities, risk, backend routes, and policy come from
  Rust types, registry/configuration, and adapter mappings.
- Unsupported devices, actions, risk states, and backends fail closed.
- Real device execution stays disabled by default.

## Local Checks

Run these before opening a pull request:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path edgehome-gate.sqlite eval cases/zh-home.yaml --gate
git diff --check
```

Use a temporary database path if you do not want local SQLite files in the
repository:

```powershell
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-gate.sqlite" eval cases\zh-home.yaml --gate
```

## Good First Contributions

- Add eval cases to `cases/zh-home.yaml`.
- Improve docs that explain the command boundary, adapter contract, or
  customization model.
- Add registry examples that keep backend-specific values outside model output.
- Add fail-closed tests for unsupported capabilities or backend mappings.
- Improve trace, replay, or eval reporting without widening public claims.

## Adding Eval Cases

When adding a case:

1. Put it in `cases/zh-home.yaml`.
2. Use a stable `id`.
3. Pick an existing category when possible.
4. Include expected intent, room, `device_id`, `device_type`, action,
   `policy_decision`, and `dry_run_ready` where applicable.
5. Add negative cases for unknown devices, unsupported capabilities, backend
   access attempts, or unsafe actions.
6. Run the release gate.

The release gate is evidence for covered harness regressions. Do not describe it
as proof of broad natural-language understanding or real-device deployment.

## Adding Devices or Capabilities

Device and capability changes should be registry-driven:

- Add or update records in `configs/devices.yaml` or an example config.
- Keep backend entity IDs, topics, and routes in config or adapter mappings.
- Add capability ranges for numeric parameters such as brightness or
  temperature.
- Add eval cases for normal, unsupported, and unsafe paths.
- Do not change the model prompt to emit vendor payloads directly.

## Adding a Backend Adapter

Before marking a backend as implemented:

1. Define the adapter contract and dry-run payload shape.
2. Load backend-specific IDs or routes from adapter configuration.
3. Keep secrets out of traces and dry-run payloads.
4. Add golden payload tests.
5. Add fail-closed tests for missing routes, invalid IDs, unsupported actions,
   and missing secrets.
6. Update README, `docs/backend-adapter-contract.md`, and `CHANGELOG.md`.

Until those steps are done, keep the backend listed as not implemented or as a
fail-closed path. For bridge adapters, do not claim real device support until a
private bridge/controller and device validation evidence exist.

## Documentation Claims

Public wording should be conservative:

- Say "gateway boundary" or "bridge adapter" when code implements translation
  but real ecosystem coverage still depends on private infrastructure.
- Say "not implemented" when code and tests do not implement the backend.
- Say "dry-run" unless real execution is explicitly implemented, configured,
  and tested.
- Separate mock eval metrics from real MiniCPM/Ollama metrics.

## Secrets

Do not commit:

- Home Assistant tokens.
- MIoT or miIO tokens.
- Private LAN URLs.
- Real device identifiers that should not be public.
- Local SQLite files produced by demos or eval runs.
