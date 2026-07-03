# EdgeHome Harness

![EdgeHome Harness overview](docs/assets/edgehome-harness-overview.jpg)

**EdgeHome Harness** is a Rust agent harness built specifically for **MiniCPM-class
1B edge models**, with the current default profile tuned for
`openbmb/minicpm5:1b`.

The project focuses on a constrained smart-home control scenario: Chinese IoT
commands, local inference, structured JSON candidates, deterministic validation,
safe execution boundaries, trace replay, and release-gated evaluation.

It is not a generic chatbot framework. It is also not a replacement for Home
Assistant, Mi Home, Matter, or a commercial smart speaker. The goal is narrower
and more engineering-oriented:

```text
Turn a small local MiniCPM model that may repeat, drift, or produce unstable JSON
into a bounded command-candidate generator.

Then let Rust decide what is valid, safe, executable, traceable, and releasable.
```

## Core Idea

```text
ModelOutput != Command
```

MiniCPM proposes a candidate JSON object. The harness decides whether that
candidate can become an `ExecutionPlan`.

The model never gets direct authority over devices, backend entity IDs, tokens,
policy, memory writes, or execution. Those responsibilities stay in deterministic
Rust components:

```text
MiniCPM / MiniCPM5-1B
+ Rust Harness
+ Runtime Memory
+ Output Governor
+ Device Registry
+ Policy Gate
+ Dry-run / ExecutionPlan boundary
+ Trace / Replay / Eval / Release Gate
```

## Why MiniCPM

EdgeHome Harness is designed around the practical constraints of **small local
models**, especially MiniCPM-style 1B deployment:

- Short context windows are preferred over long conversational history.
- JSON output must be short, bounded, and schema checked.
- Repetition and dead-loop behavior must be detected outside the model.
- Runtime memory should be compact, structured, and optional under pressure.
- Safety policy must not depend on model judgment.
- Unknown devices, unsupported actions, and dangerous operations must fail
  closed.

The default runtime profile is intentionally MiniCPM-oriented:

```yaml
model_name: openbmb/minicpm5:1b
temperature: 0.1
top_p: 0.8
top_k: 20
repeat_penalty: 1.25
num_ctx: 1024
num_predict: 128
timeout_ms: 8000
retry_count: 1
memory_enabled: true
max_short_memory_turns: 3
max_context_chars: 500
executor_backend: mock
```

The architecture can be adapted to other small structured-output models, but the
current project claim is deliberately scoped to MiniCPM/MiniCPM5-style local
edge inference.

## What This Project Demonstrates

EdgeHome Harness demonstrates the engineering layer required before a small
model is allowed to participate in a real execution chain:

- Guard user input before it reaches the model.
- Compile compact runtime memory instead of injecting full chat history.
- Ask MiniCPM to generate candidate JSON only.
- Govern model output length, malformed JSON, retries, and repetition.
- Parse and validate candidate JSON against Rust-owned schemas.
- Normalize semantic slots into internal command types.
- Resolve device aliases through a registry, not through the model.
- Enforce capability boundaries such as `light.set_brightness` vs.
  unsupported `light.set_temperature`.
- Apply deterministic policy for low, medium, high, blocked, and unknown risk.
- Produce dry-run plans before any real execution.
- Record evidence, traces, audit events, and replayable failure frames.
- Evaluate releases against regression gates such as false-allow rate and
  fail-closed coverage.

## Current Status

This repository is a reproducible engineering prototype and interview-ready
demonstration project.

Implemented:

- Rust workspace with separated crates for core types, parser, registry, gate,
  memory, Ollama adapter, executor, storage, trace, eval, and CLI.
- MiniCPM5-1B low-memory profile.
- Mock model and mock executor path for deterministic demos.
- Ollama structured-output request adapter.
- Output governor for overlong output, invalid JSON, dead-loop detection, retry
  policy, and fallback classification.
- Runtime memory for short-session references and confirmed long-term aliases.
- Device registry and capability validation.
- Policy gate with fail-closed behavior.
- Dry-run execution planner.
- Home Assistant demo backend boundary.
- SQLite-backed evidence, audit, trace, replay, and long-term memory.
- Evaluation cases and release gate metrics.
- Low-memory pressure policy for context/output reduction and rule-only fallback.

Not claimed:

- Production-ready smart-home gateway.
- Replacement for Mi Home, Home Assistant, Matter, MQTT, or a smart speaker.
- Full Mi Home / MIoT / miIO / Matter integration.
- Long-running benchmark on a real 2GB ARM board.
- Proof that all smart-home natural-language inputs are understood.
- Real-device execution enabled by default.

## Architecture

```mermaid
flowchart TD
    A["Chinese smart-home command"] --> B["Input Guard"]
    B --> C["Rule Pre-Parser"]
    C --> D["Runtime Memory"]
    D --> E["Context Compiler"]
    E --> F["MiniCPM5-1B Candidate JSON"]
    F --> G["Output Governor"]
    G --> H["JSON / Schema Validator"]
    H --> I["Semantic Normalizer"]
    I --> J["Device Registry"]
    J --> K["Policy Gate"]
    K --> L["Dry-run ExecutionPlan"]
    L --> M["Executor Boundary"]
    M --> N["Trace / Replay / Eval"]
```

The model is deliberately kept near the top of the pipeline. Everything after
candidate generation is owned by the harness.

## Online Request Path

A single request goes through this shape:

```text
1. User input is checked by Input Guard.
2. High-confidence commands may be handled by the Rule Pre-Parser.
3. Runtime Memory provides compact recent context and confirmed aliases.
4. Context Compiler builds a short MiniCPM prompt.
5. MiniCPM / Ollama generates candidate JSON.
6. Output Governor rejects overlong, repetitive, malformed, or unstable output.
7. JSON Parser and Schema Validator validate structure.
8. Semantic Normalizer converts candidate fields into internal command types.
9. Device Registry resolves aliases and checks known devices.
10. Capability Gate verifies that the device supports the requested action.
11. Policy Gate decides allow, require confirmation, or deny.
12. DryRunPlanner creates an ExecutionPlan if policy permits.
13. Executor boundary remains disabled by default for real devices.
14. Trace Recorder writes replayable evidence and audit data.
15. Eval Gate uses traces and command results to prevent release regressions.
```

## Safety Model

The harness treats every model output as untrusted.

Examples of deterministic safety rules:

- Unknown intent fails closed.
- Unknown room fails closed.
- Unknown device fails closed.
- Missing `device_id` fails closed.
- Unsupported capability fails closed.
- Out-of-range brightness fails closed.
- Blocked-risk device actions are denied.
- Medium/high-risk actions require confirmation before real execution.
- Prompt-injection-like input is flagged.
- Backend access requests such as `entity_id`, local URLs, SSH, and tokens are
  flagged.
- Real execution is disabled by default.

Example denied request:

```text
关闭燃气报警器
```

The mock pipeline normalizes it as a gas-device command, resolves the registry
entry, then rejects it because the authoritative device risk is `blocked`.

## Runtime Memory

Memory is intentionally small and structured. It is not an unbounded chat log.

Short-session memory handles references such as:

```text
把刚才那个灯再调暗一点
关闭空调
```

Long-term memory only accepts explicit and confirmed writes, for example:

```text
以后把玄关灯叫小夜灯
```

Memory cannot weaken safety policy. The context compiler also enforces character
budgets so low-resource devices do not keep growing prompts.

## Low-Memory Profile

The default profile targets constrained edge environments:

```text
2GB-4GB RAM
local inference
short context
short JSON output
bounded memory injection
deterministic policy
traceable failure
```

The resource pressure policy can reduce `num_ctx` and `num_predict`, and in
critical pressure it can disable memory injection and switch to rule-only
fallback.

See:

- [2GB RAM Profile](docs/2gb-profile.md)
- [2GB Memory Budget](docs/2gb-memory-budget.md)
- [Model Parameters](docs/model-parameters.md)

## Quick Start

Requirements:

- Rust `1.95` or newer.
- Optional: Ollama with `openbmb/minicpm5:1b` for real MiniCPM runs.

Show the default low-memory configuration:

```powershell
cargo run -q -p edgehome-cli -- config show
```

Run a deterministic mock dry-run:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite dry-run --mock "把客厅灯打开"
```

Run a denied safety case:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-demo.sqlite dry-run --mock "关闭燃气报警器"
```

Run the mock evaluation suite:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-eval.sqlite eval cases/zh-home.yaml --gate
```

Run the demo script:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\demo.ps1 -DatabasePath edgehome-demo.sqlite
```

## Running With MiniCPM Through Ollama

The mock path is for deterministic demos and regression tests. To exercise the
model path, start Ollama and make sure the configured model is available:

```powershell
ollama pull openbmb/minicpm5:1b
ollama serve
```

Then run without `--mock`:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-ollama.sqlite dry-run "把客厅灯打开"
```

The Ollama response still goes through Output Governor, parser/schema
validation, registry checks, policy gates, dry-run planning, and trace recording.

## Evaluation Baseline

The included eval suite is intentionally small but covers the main harness
properties:

```text
normal_control
runtime_memory
slot_extraction
long_memory
confirmation_policy
high_risk_policy
fail_closed
capability_boundary
unknown_device
input_guard
```

The current release gate checks:

```text
total_cases >= 10
category_count >= 8
pass_rate >= 1.0
schema_valid_rate >= 1.0
dead_loop_rate <= 0.0
trace_coverage >= 1.0
intent_accuracy >= 0.95
slot_accuracy >= 0.90
input_guard_flag_accuracy >= 1.0
false_allow_rate <= 0.0
fail_closed_rate >= 1.0
retry_rate <= 0.30
```

Important: the mock eval proves harness behavior and regression coverage. It is
not a broad natural-language understanding benchmark. Real MiniCPM evaluation
should be run separately and reported with model output, latency, retry, and
fallback metrics.

## Home Assistant Boundary

Home Assistant is supported as a demo backend boundary, not as the project
itself.

The model never sees:

```text
Home Assistant token
entity_id
service path
backend route
private network URL
```

Device routing is owned by the registry and executor. Real execution remains
disabled by default and should only be enabled for explicit local testing after
dry-run, gate, and confirmation checks.

See:

- [Home Assistant Demo](docs/home-assistant-demo.md)
- [Deployment Modes](docs/deployment-modes.md)

## Repository Layout

```text
crates/edgehome-core       core schemas and command types
crates/edgehome-config     runtime profiles and low-memory config loading
crates/edgehome-parser     input guard, JSON extraction, schema validation, normalizer
crates/edgehome-registry   device registry, alias resolution, capability checks
crates/edgehome-gate       deterministic policy and execution gates
crates/edgehome-memory     short-session memory and confirmed long-term memory
crates/edgehome-ollama     MiniCPM/Ollama adapter and output governor
crates/edgehome-executor   dry-run planner, mock executor, Home Assistant boundary
crates/edgehome-storage    SQLite-backed evidence storage
crates/edgehome-trace      trace, audit, replay frame types
crates/edgehome-eval       case loading, metrics, release gate
crates/edgehome-cli        command-line demo and eval runner

cases/                     regression cases
configs/                   runtime and device configuration
docs/                      architecture, demo, deployment, and memory notes
scripts/                   demo and embedded validation scripts
```

## Development Checks

Run tests:

```powershell
cargo test --workspace
```

Run formatting:

```powershell
cargo fmt --all --check
```

Run Clippy:

```powershell
cargo clippy --workspace --all-targets -- -D warnings
```

Run the release gate:

```powershell
cargo run -q -p edgehome-cli -- --db-path edgehome-gate.sqlite eval cases/zh-home.yaml --gate
```

## Design Positioning

EdgeHome Harness is not trying to make MiniCPM act like a large cloud agent. It
uses MiniCPM where a small local model is useful: converting natural language
into a compact candidate. Everything that must be reliable is moved back into
Rust.

The intended message is:

```text
Small model.
Strong harness.
Safe execution.
```

That is the core value of this repository.
