# EdgeHome Harness Plan

## 1. Project Goal

Build a Rust-based harness for 1B edge language models that can parse smart home and IoT commands into safe, validated, auditable actions under 2GB RAM constraints.

The project focuses on:

```text
1B edge small models
smart home / IoT commands
JSON / intent parsing
business validation
local execution safety
2GB RAM stability
MiniCPM5 / Qwen evaluation
```

The first runtime integration will be Ollama structured outputs.

The first model comparison will be:

```text
MiniCPM5-1B
Qwen3.5-0.8B
```

## 2. Key Principle

Ollama structured output guarantees JSON syntax. EdgeHome Harness guarantees local action safety.

```text
Ollama JSON constraint:
  validates format

EdgeHome Harness:
  validates business meaning
  validates device capability
  enforces policy
  prevents unsafe execution
  prevents runaway runtime behavior
  provides audit logs
```

The project should always treat model output as untrusted.

## 3. MVP Definition

The MVP is complete when this command works end to end:

```bash
edgehome dry-run "晚上十点后把走廊灯调到30%"
```

Expected output:

```json
{
  "intent": "create_rule",
  "room": "hallway",
  "device": "light",
  "action": "set_brightness",
  "brightness": 30,
  "time_after": "22:00",
  "policy": "allow",
  "dry_run": true
}
```

The MVP must include:

```text
Rust CLI
Ollama adapter
model output cleaner
JSON extraction
schema validation
semantic normalization
business validation
policy decision
mock dry-run executor
SQLite audit log
YAML eval runner
MiniCPM5 vs Qwen test cases
```

The MVP must not include:

```text
real door lock control
real gas device control
multi-step autonomous agent loops
cloud dependency
Node.js daemon
Python production service
```

## 4. Milestones

### Milestone 0: Repository Foundation

Tasks:

```text
create Rust workspace
create crate layout
add formatter and lint config
add README and plan
add initial CI later
```

Workspace layout:

```text
crates/
  edgehome-core/
  edgehome-ollama/
  edgehome-parser/
  edgehome-policy/
  edgehome-executor/
  edgehome-audit/
  edgehome-eval/
  edgehome-cli/
  edgehome-server/
```

Acceptance criteria:

```text
cargo check passes
edgehome CLI binary builds
```

### Milestone 1: Core Types and Schema

Tasks:

```text
define Intent enum
define Room enum
define Device enum
define Action enum
define RiskLevel enum
define Command struct
define ModelCandidate struct
define PolicyDecision enum
define ExecutionPlan struct
```

Initial intents:

```text
turn_on
turn_off
set_brightness
set_temperature
query_status
create_rule
unknown
```

Initial rooms:

```text
living_room
bedroom
hallway
kitchen
bathroom
unknown
```

Initial devices:

```text
light
air_conditioner
curtain
camera
lock
unknown
```

Acceptance criteria:

```text
all core types serialize and deserialize
JSON schema can be generated or maintained for Ollama format
unit tests cover enum parsing and serialization
```

### Milestone 2: Ollama Structured Output Adapter

Tasks:

```text
call Ollama /api/chat
support model name config
support JSON schema format
support timeout
support temperature/top_p/top_k/repeat_penalty/num_predict
support non-streaming first
capture raw model output
```

Default models:

```text
openbmb/minicpm5:q4_K_M
qwen3.5:0.8b or local equivalent
```

Acceptance criteria:

```text
edgehome parse can call Ollama
raw output is stored
model errors are returned as structured errors
timeout works
```

### Milestone 3: Output Cleaner and Parser

Tasks:

```text
strip <think>...</think>
strip markdown code fences
extract first JSON object
handle extra text before/after JSON
fail closed when no JSON is found
```

Examples to handle:

```text
<think>...</think>{"intent":"turn_off"}
```json
{"intent":"turn_off"}
```
Here is the JSON: {"intent":"turn_off"}
```

Acceptance criteria:

```text
cleaner unit tests pass
invalid output does not panic
invalid output does not execute
```

### Milestone 4: Semantic Normalization

Tasks:

```text
Chinese room mapping
Chinese device mapping
Chinese action mapping
brightness extraction
temperature extraction
time expression normalization
device alias handling
```

Examples:

```text
走廊 -> hallway
客厅 -> living_room
走廊灯 -> room=hallway, device=light
关掉 -> turn_off
调到 -> set_brightness when target is light
30% -> 30
晚上十点后 -> 22:00
```

Acceptance criteria:

```text
normalization tests cover common Chinese smart-home commands
model raw Chinese output can be converted into canonical command format
```

### Milestone 5: Business Validator

Tasks:

```text
device registry
device capability table
value range validation
room/device existence checks
action compatibility checks
```

Rules:

```text
light supports turn_on, turn_off, set_brightness
air_conditioner supports turn_on, turn_off, set_temperature
curtain supports open, close, set_position later
lock requires confirmation for lock/unlock
camera requires confirmation for disable
gas devices are blocked
```

Acceptance criteria:

```text
invalid device/action combinations are rejected
missing devices are rejected
out-of-range values are rejected
```

### Milestone 6: Policy Engine

Tasks:

```text
assign risk levels
make allow/confirm/deny decisions
support policy config
support deny-by-default for unknown actions
```

Risk model:

```text
read -> allow
low -> allow
medium -> audit or confirm
high -> require confirmation
blocked -> deny
```

Acceptance criteria:

```text
lock/camera actions require confirmation
gas/medical/critical devices are denied
unknown devices and unknown actions are denied
```

### Milestone 7: Mock Executor and Dry Run

Tasks:

```text
create Executor trait
implement MockExecutor
implement dry-run output
block direct execution by default
```

Acceptance criteria:

```text
edgehome dry-run returns executable plan
edgehome execute refuses high-risk commands unless explicitly confirmed
MVP does not control real devices
```

### Milestone 8: Audit Log

Tasks:

```text
SQLite audit database
store raw input
store model name
store raw model output
store cleaned output
store normalized command
store validation result
store policy decision
store dry-run result
store latency
```

Acceptance criteria:

```text
every parse/dry-run writes an audit record
audit records can be queried by CLI
```

### Milestone 9: Eval Runner

Tasks:

```text
read YAML cases
run parse pipeline
compare expected command fields
compute metrics
compare MiniCPM5 and Qwen
export JSON/Markdown report
```

Metrics:

```text
intent accuracy
room accuracy
device accuracy
action accuracy
slot accuracy
schema valid rate
normalization success rate
policy correctness
latency average
latency p95
dead-loop or timeout count
```

Acceptance criteria:

```text
edgehome eval cases/zh-home.yaml runs
report compares MiniCPM5-1B and Qwen3.5-0.8B
```

### Milestone 10: 2GB RAM Profile

Tasks:

```text
document test environment
run with MiniCPM5-1B Q4
run with Qwen3.5-0.8B
measure RSS and latency
record context length and num_predict settings
test under constrained memory
```

Target settings:

```text
num_ctx=1024
num_predict=80-128
temperature=0-0.2
headless Linux
no desktop UI
Rust daemon
model sidecar
```

Acceptance criteria:

```text
2GB profile documented
memory peak recorded
latency recorded
failure modes documented
```

## 5. Initial Test Cases

Create `cases/zh-home.yaml` with examples:

```yaml
- input: "把客厅灯关掉"
  expected:
    intent: turn_off
    room: living_room
    device: light
    action: turn_off

- input: "把卧室空调调到26度"
  expected:
    intent: set_temperature
    room: bedroom
    device: air_conditioner
    action: set_temperature
    temperature: 26

- input: "晚上十点后把走廊灯调到30%"
  expected:
    intent: create_rule
    room: hallway
    device: light
    action: set_brightness
    brightness: 30
    time_after: "22:00"

- input: "打开前门门锁"
  expected_policy: require_confirmation

- input: "关闭所有摄像头"
  expected_policy: require_confirmation

- input: "关闭燃气报警器"
  expected_policy: deny
```

## 6. Development Order

Recommended order:

```text
1. Rust workspace and core types
2. CLI skeleton
3. Ollama adapter
4. output cleaner
5. schema validation
6. semantic normalization
7. policy engine
8. mock dry-run executor
9. audit log
10. eval runner
11. model comparison report
12. 2GB RAM profile
```

Do not implement real device control before policy, audit, and eval are working.

## 7. Model Strategy

MiniCPM5-1B is the primary pure-text candidate.

Qwen3.5-0.8B is the comparison candidate and may be useful when multimodal support is required.

The harness must assume:

```text
model output is untrusted
model output can contain think blocks
model output can contain markdown
model output can be semantically wrong
model output can be slow or timeout
model output can be valid JSON but unsafe
```

## 8. First Success Criteria

The first real success is not a chat demo.

The first real success is:

```text
edgehome eval cases/zh-home.yaml
```

showing:

```text
MiniCPM5-1B and Qwen3.5-0.8B comparison
valid JSON rate
intent/slot accuracy
policy correctness
latency
memory profile
```

The second success is:

```text
edgehome dry-run "晚上十点后把走廊灯调到30%"
```

returning a safe execution plan without controlling a real device.

## 9. Later Extensions

After MVP:

```text
Home Assistant adapter
MQTT adapter
HTTP device adapter
confirmation workflow
local web dashboard
llama.cpp runtime adapter
GBNF grammar support
systemd deployment
cross-compile for ARM Linux
2GB board benchmark
```

Do not add these before the MVP pipeline is stable.

