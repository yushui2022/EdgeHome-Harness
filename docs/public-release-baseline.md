# Public Release Baseline

This document defines the current public baseline for EdgeHome Harness. It is
intended for GitHub releases, WAIC one-page material, demos, and external
writeups.

The baseline is evidence-driven: public language should point to commands,
tests, manifests, and explicit claim boundaries instead of relying on broad
phrasing.

## Baseline Positioning

Use this positioning:

```text
EdgeHome Harness is a Rust safety harness for MiniCPM-powered edge home command
agents. MiniCPM proposes backend-neutral candidate JSON, Rust validates and
gates commands, and backend adapters translate verified plans into backend
payloads.
```

Short form:

```text
MiniCPM proposes. Rust decides. Adapters translate.
```

## Baseline Capabilities

The current public baseline supports these claims:

| Area | Public baseline |
| --- | --- |
| Model role | MiniCPM/Mock proposes backend-neutral candidate JSON only. |
| Harness role | Rust owns schema validation, normalization, registry resolution, policy/capability gates, trace, replay, and eval. |
| Execution boundary | Dry-run by default; real execution requires explicit private opt-in. |
| Eval gate | 108 deterministic mock cases across 12 categories. |
| Safety metrics | `false_allow_rate = 0.0` and `fail_closed_rate = 1.0` on the release gate. |
| Home Assistant | Gateway boundary with dry-run payloads, route validation, opt-in REST execution, and optional post-state readback. |
| MQTT | Dry-run route translation and guarded publish executor; public configs keep execution disabled. |
| MIoT/Xiaomi | Bridge request adapter and opt-in bridge executor; real Xiaomi support requires private bridge/device evidence. |
| Matter | Controller-bridge request adapter and opt-in bridge executor; real control requires private controller/device evidence. |
| Evidence bundle | Git-ignored report, JSON artifacts, SHA-256 manifest, manifest schema, and standalone verifier. |

## Required Public Evidence

Before publishing release material, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release-check.ps1 -WithDemoSmoke
```

This command must pass:

- release hygiene scan;
- JSON schema syntax check;
- public claim lint;
- `cargo fmt --all --check`;
- `cargo clippy --locked --workspace --all-targets -- -D warnings`;
- `cargo test --locked --workspace`;
- 108-case release eval gate;
- backend readiness checks for Home Assistant, MQTT, MIoT bridge, and Matter
  bridge;
- `git diff --check`;
- public demo evidence generation;
- standalone demo evidence manifest verification.

For a reviewed public snapshot, regenerate the demo bundle from a clean tracked
worktree and verify it strictly:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\verify-demo-evidence.ps1 `
  -EvidenceDir artifacts\release-demo-smoke `
  -RequireCleanManifest
```

The strict verifier requires:

- manifest schema version `edgehome.demo_evidence_manifest.v1`;
- `tracked_worktree_dirty = false`;
- `model_mode = mock`;
- `real_device_execution = disabled_by_default`;
- expected artifact files present and non-empty;
- byte counts matching the manifest;
- SHA-256 hashes matching the manifest;
- relative artifact file names only.

## Public Bundle Contents

The public demo bundle contains:

| File | Evidence |
| --- | --- |
| `public-demo-report.md` | Human-readable demo summary and claim boundary. |
| `01-release-gate.json` | Release gate metrics and pass/fail checks. |
| `02-ordinary-dry-run.json` | Ordinary command dry-run evidence. |
| `03-slot-dry-run.json` | Slot extraction dry-run evidence. |
| `04-replay-summary.json` | Replay summary for a recorded trace. |
| `05-trace-frame.json` | Exported trace frame. |
| `06-short-memory.json` | Short-memory relative command evidence. |
| `07-long-memory.json` | Explicit long-memory alias evidence. |
| `08-dangerous-blocked.json` | Dangerous command fail-closed evidence. |
| `09-low-memory-pressure.json` | Runtime pressure and fallback behavior. |
| `10-backend-readiness.json` | Read-only adapter readiness checks. |
| `11-output-governor-test.txt` | Focused output-governor test output. |
| `12-evidence-manifest.json` | Git commit, dirty flag, artifact sizes, SHA-256 hashes, and claim boundary. |

## Non-Claims

Do not use the public baseline to claim:

- production-ready smart-home gateway status;
- universal Xiaomi, MIoT, or Matter device support;
- real Xiaomi or Matter hardware validation;
- a Home Assistant or Mi Home replacement;
- default-on real-device execution;
- MiniCPM-generated vendor-ready JSON;
- broad natural-language understanding from mock eval metrics;
- proven physical 2GB ARM long-running deployment.

## Upgrade Path

To upgrade this baseline, add evidence first:

1. Extend or implement the relevant adapter behavior.
2. Add golden payload, fail-closed, redaction, and execution-boundary tests.
3. Add or update private config examples without committing secrets.
4. Generate real-device or private-bridge evidence when claiming real-device
   support.
5. Update `docs/public-claims.md`, `docs/release-evidence.md`,
   `docs/release-checklist.md`, and this baseline.
6. Run `scripts\release-check.ps1 -WithDemoSmoke`.
7. Regenerate a clean demo evidence bundle and verify it with
   `-RequireCleanManifest`.
