# EdgeHome Harness

EdgeHome Harness is a Rust-first local action harness for 1B edge language models in smart home and IoT scenarios.

It turns small local models such as MiniCPM5-1B and Qwen3.5-0.8B into constrained, validated, auditable command parsers for home and edge devices under tight memory budgets such as 2GB RAM.

This project is not a general chatbot framework. It is not trying to make a 1B model behave like a cloud-scale agent. Its goal is narrower and more practical:

```text
natural language command
  -> structured intent / JSON candidate
  -> semantic normalization
  -> business validation
  -> local safety policy
  -> dry-run / execution
  -> audit log
```

## Core Idea

Ollama structured output and JSON schema constraints solve one layer:

```text
Ollama JSON constraint = syntax correctness
```

It can help prevent malformed JSON. It does not guarantee that the command is safe, legal, executable, authorized, or stable on a constrained edge device.

EdgeHome Harness solves the higher-level runtime and business layer:

```text
EdgeHome Harness = business safety + device control + local reliability
```

Both are needed. They do not conflict.

Use Ollama structured outputs to make the model produce valid JSON. Use EdgeHome Harness to decide whether that JSON is meaningful, safe, executable, and worth trusting.

## Why JSON Constraint Is Not Enough

Here are five real problems that JSON schema constraints alone cannot solve.

### 1. Valid JSON Can Still Be Dangerous

The model may output perfectly valid JSON:

```json
{
  "intent": "unlock",
  "device": "front_door_lock",
  "room": "entrance"
}
```

The JSON is syntactically valid. That does not mean the action should execute.

The harness must decide:

```text
door lock action -> high risk -> require confirmation or deny
```

### 2. Valid JSON Can Be Semantically Wrong

The model may output:

```json
{
  "device": "light",
  "action": "set_temperature",
  "value": 26
}
```

This is valid JSON, but it is an invalid device action. Lights do not support temperature control.

The harness must validate the command against device capability metadata.

### 3. Valid JSON Can Target a Missing or Offline Device

The model may output:

```json
{
  "room": "hallway",
  "device": "light",
  "action": "set_brightness",
  "brightness": 30
}
```

The JSON is valid, but the hallway light may not exist, may be offline, or may be controlled by a different integration.

The harness must resolve devices against the local device registry before execution.

### 4. Valid JSON Can Create Unsafe Automation

The model may output a valid automation rule:

```json
{
  "intent": "create_rule",
  "condition": "after_22_00",
  "action": "turn_off_all_devices"
}
```

That may accidentally turn off medical devices, security cameras, network equipment, or other critical devices.

The harness must apply policy and exclusions before any automation is accepted.

### 5. Valid JSON Does Not Prevent Runtime Failure

The model can still:

```text
run too long
repeat internally
return slow responses
consume too much memory
trigger retries forever
block the device control loop
crash the local model sidecar
```

Ollama JSON constraints do not solve runtime stability.

The harness must enforce:

```text
timeouts
token limits
memory budgets
retry limits
fallback paths
dead-loop detection
audit logging
health checks
```

## Target Scope

The first target scope is deliberately narrow:

```text
1B edge language models
smart home / IoT Chinese commands
JSON / intent harness
local execution safety
2GB RAM deployment constraints
MiniCPM5 / Qwen small-model evaluation
```

The initial models:

```text
MiniCPM5-1B
Qwen3.5-0.8B
Qwen3-0.6B, optional fallback baseline
```

The initial runtime:

```text
Ollama structured outputs
```

Future runtime adapters may include:

```text
llama.cpp server
llama.cpp grammar / GBNF
ONNX Runtime
device-vendor runtimes
```

## Non-Goals

This project does not aim to be:

```text
a general chatbot
a LangChain clone
a full autonomous agent framework
a model training framework
a cloud LLM platform
a replacement for Home Assistant
a direct real-time device controller
```

The harness should not allow a small model to directly control high-risk devices.

The model proposes. The harness validates. The policy layer decides. The executor acts only when allowed.

## Architecture

```text
User Input
  |
  v
Normalizer
  |
  v
Rule Pre-Parser
  |
  v
Task Router
  |
  v
Model Adapter
  - Ollama structured outputs
  - MiniCPM5-1B / Qwen3.5-0.8B
  |
  v
Output Cleaner
  - strip <think> blocks
  - strip markdown fences
  - extract JSON object
  |
  v
Schema Validator
  |
  v
Semantic Normalizer
  - Chinese room/device/action mapping
  - number and time normalization
  |
  v
Business Validator
  - device exists
  - capability exists
  - value range is legal
  |
  v
Policy Engine
  - allow
  - require confirmation
  - deny
  |
  v
Dry-Run Executor
  |
  v
Audit Log
```

## Example

Input:

```text
晚上十点后把走廊灯调到 30%
```

Model candidate:

```json
{
  "room": "走廊",
  "device": "走廊灯",
  "action": "调到",
  "brightness": "30%",
  "time_after": "晚上十点后"
}
```

Harness-normalized command:

```json
{
  "intent": "create_rule",
  "room": "hallway",
  "device": "light",
  "action": "set_brightness",
  "brightness": 30,
  "time_after": "22:00",
  "risk": "low",
  "policy": "allow"
}
```

Dry-run execution plan:

```json
{
  "dry_run": true,
  "target": "home.light.hallway",
  "action": "set_brightness",
  "brightness": 30,
  "condition": {
    "time_after": "22:00"
  }
}
```

## Rust Workspace Plan

The project will be implemented as a Rust workspace.

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

Initial command-line interface:

```bash
edgehome parse "把客厅灯关掉"
edgehome dry-run "晚上十点后把走廊灯调到30%"
edgehome eval cases/zh-home.yaml
edgehome serve
```

## Evaluation First

The project should be test-driven around small-model behavior.

Evaluation cases will measure:

```text
intent accuracy
room accuracy
device accuracy
action accuracy
slot accuracy
JSON valid rate
normalization success rate
policy decision correctness
dead-loop rate
latency
memory usage
2GB deployment stability
```

The core comparison will be:

```text
MiniCPM5-1B vs Qwen3.5-0.8B
```

The project should not claim model quality without local benchmark results.

## Safety Model

Actions are divided into risk levels:

```text
read      -> query status, safe
low       -> lights, brightness, curtains
medium    -> air conditioner, appliance power
high      -> locks, cameras, security system
blocked   -> gas, medical devices, critical infrastructure
```

Policy examples:

```text
read      -> allow
low       -> allow or dry-run
medium    -> allow with audit or require confirmation
high      -> require confirmation
blocked   -> deny
```

## 2GB RAM Assumptions

The target deployment profile is:

```text
headless Linux
Rust agent daemon
Ollama or llama.cpp model sidecar
MiniCPM5-1B Q4 or similar 1B model
small context window
bounded output length
SQLite audit log
no Node.js daemon
no Python production daemon
```

The system must treat memory as a first-class constraint.

## License

License is not decided yet.

Candidate:

```text
Apache-2.0
MIT
```

