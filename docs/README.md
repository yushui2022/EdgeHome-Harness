# EdgeHome Harness Docs

This directory contains engineering notes, claim boundaries, demo guides, and
release evidence for EdgeHome Harness.

Core documents:

```text
architecture-v2.md
  V2 architecture: runtime memory, trace/replay/eval observability, and the main
  command pipeline.

command-pipeline-contract.md
  Boundaries between model candidate JSON, normalized command, ExecutionPlan,
  and backend adapter payloads.

backend-adapter-contract.md
  Mock, Home Assistant, MQTT, MIoT bridge, and Matter bridge adapter contracts,
  fail-closed rules, and explicit execute rules.

bridge-api-contract.md
  Private MIoT/Matter bridge HTTP API shape, error semantics, and redaction
  boundary.

schemas/demo-evidence-manifest.schema.json
  JSON Schema for the generated public demo evidence manifest.

customization.md
  How users customize devices, capabilities, and backend mappings while keeping
  the model output schema fixed.

roadmap.md
  Current baseline, near-term work, adapter order, hardware evidence
  requirements, and non-goals.

release-checklist.md
  Required checks, gate thresholds, docs review, and secrets review before
  release.

release-evidence.md
  Reproducible release evidence boundary, demo smoke command, artifact index,
  standalone manifest verifier, and safe public claim language.

public-claims.md
  Public claim policy, allowed claims, blocked overclaims, and evidence required
  before upgrading Xiaomi, Matter, Home Assistant, MQTT, hardware, or production
  readiness language.

public-release-baseline.md
  Current public release baseline for GitHub releases, WAIC one-page material,
  demos, evidence bundles, and claim boundaries.

2gb-profile.md
  2GB RAM constraints, low_memory profile, and runtime pressure degradation.

2gb-memory-budget.md
  2GB RAM budget, module limits, headroom, and measurement commands.

model-parameters.md
  MiniCPM5-1B / Ollama parameters, output governance, and tuning order.

deployment-modes.md
  Deployment modes and Home Assistant integration boundaries.

demo-walkthrough.md
  Public demo sequence, expected signals, speaker track, and non-claims.

home-assistant-demo.md
  Home Assistant demo backend, token handling, dry-run payloads, explicit
  execute boundary, and local verification.

home-assistant-gateway.md
  Home Assistant gateway boundary, real execution switch, and post-state readback.

home-assistant-golden-payloads.md
  Home Assistant dry-run golden payloads and claim boundary.

mqtt-guarded-publish.md
  MQTT dry-run payload and guarded publish executor.

miot-bridge-adapter.md
  MIoT/Xiaomi bridge request adapter and real-device validation boundary.

matter-bridge-adapter.md
  Matter controller bridge adapter and non-goals.

eval-report-example.md
  Eval/replay sample report and explanation language.

real-minicpm-eval-report.md
  Real MiniCPM/Ollama eval report workflow and reviewed result.

qemu-embedded-validation-report.md
  QEMU 2GB embedded pre-validation notes and current limitations.

small-model-harness-blog.md
  External blog draft about why the small-model harness exists.

waic-one-page.md
  One-page public positioning, evidence, current boundaries, and adapter roadmap.
```

Documentation rules:

```text
Do not store real tokens in docs or configs.
Do not describe Home Assistant as the project itself.
Do not claim universal Xiaomi/MIoT support.
Do not claim universal Matter controller support.
Do not claim MQTT real broker publish is enabled by default.
Do not claim the model outputs vendor-ready JSON.
Keep public claims aligned with code, tests, eval, and trace evidence.
```
