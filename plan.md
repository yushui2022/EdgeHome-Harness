# EdgeHome Harness 商用级完善执行计划

最后更新：2026-07-04

本文档是后续接手 EdgeHome Harness 的权威计划文件。上下文压缩、换线程或中断后，先读本文件，再继续执行。

目标不是把 README 写得更好看，而是把项目从当前的可验证 prototype 推进到可对外展示、可复现、可审计、可扩展、接近商用工程标准的版本。

核心路线保持不变：

```text
MiniCPM proposes.
Rust decides.
Adapters translate.
```

当前 baseline：

```text
已实现：
MiniCPM/Mock candidate JSON
  -> Rust schema validation / normalization
  -> DeviceRegistry / DeviceResolver
  -> GateEngine::verify
  -> GatedCommand
  -> DryRunPlanner::plan_gated
  -> Mock / Home Assistant / MQTT dry-run payload
  -> trace / replay / eval gate

未完成但纳入最终路线：
1. 真实 MiniCPM/Ollama eval report
2. 更完整的 Home Assistant demo config + golden payload 展示
3. MQTT guarded real publish
4. MIoT/Xiaomi adapter
5. Matter controller adapter
6. 商用级真实执行模式
7. 完整生产级 Home Assistant gateway 边界
```

重要纠偏：

```text
真实设备执行不能默认开启。

商用级不是 default-on execution。
商用级是 explicit opt-in execution：
  dry-run first
  gate accepted
  risk-aware confirmation
  secret isolation
  idempotency
  rate limit
  audit log
  post-state verification
  rollback / disable switch
```

因此本计划把“真实设备执行默认开启”改成：

```text
真实设备执行可显式启用，但默认关闭。
所有真实执行都必须经过 dry-run、gate、confirmation、rate limit、audit 和 post-state verification。
```

---

## 0. 每次恢复执行先做什么

在仓库根目录运行：

```powershell
cd C:\Users\xiaoy\Desktop\edge-home\EdgeHome-Harness
git status --short
git log -5 --oneline
```

如果有未提交改动：

```powershell
git diff --stat
git diff
```

不要回滚不认识的改动。先判断改动属于哪个里程碑，再继续。

每个里程碑完成前必须运行：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-release-gate.sqlite" eval cases\zh-home.yaml --gate
git diff --check
```

提交节奏：

```text
一个独立能力点通过验证后 commit。
每个 commit 后 push。
不要把 README、adapter 代码、eval 扩容、质量门修复混成一个 commit。
```

建议 commit message：

```text
docs: update commercial roadmap
eval: add real minicpm report workflow
docs: expand home assistant demo boundary
executor: add mqtt dry-run adapter
executor: add mqtt execution guard
docs: define miot adapter contract
docs: define matter controller contract
executor: harden explicit execution mode
```

---

## 1. 不变量

这些规则不能因为要“商用级”而放松。

### 1.1 ModelOutput != Command

MiniCPM 输出永远只是候选 JSON，不是命令。

模型可以提出：

```text
intent
room
device_alias
device_type
action
params
```

模型不能决定：

```text
真实 device_id
Home Assistant entity_id
MIoT did / siid / piid / aiid
Matter node / endpoint / cluster / command id
MQTT topic
backend URL
token
真实执行开关
风险等级
```

这些只能来自：

```text
Rust types
DeviceRegistry
Capability rules
Policy config
Backend adapter config
显式用户或管理员配置
```

### 1.2 模型输出 schema 固定，后端映射可定制

不要把项目做成：

```text
让小模型按不同厂家要求输出任意 JSON。
```

正确架构：

```text
固定 canonical candidate JSON
  -> Rust validation / normalization
  -> internal ExecutionPlan
  -> backend adapter payload
```

对外说法：

```text
The model output contract is fixed and safe.
The device registry and backend adapter mappings are customizable.
```

### 1.3 未实现 backend 必须 fail closed

任何未实现、未配置、缺 secret、缺 route、能力不匹配、风险过高的路径都必须失败关闭。

禁止：

```text
backend 不支持时回退到 mock
缺配置时猜 topic / entity_id / did
模型输出里直接带 vendor payload
```

### 1.4 真实执行必须显式启用

默认行为：

```text
dry-run only
real execution disabled
secrets absent
no backend network call
```

真实执行允许条件：

```text
operator explicitly enables execution
dry-run plan exists
GateEngine accepted
policy allows or user confirmed required risk
rate limit passed
idempotency passed
secret loaded from env/file outside git
post-state verification configured
audit event recorded
```

---

## 2. 商用级最终目标

最终版不是“所有智能家居都能控制”，而是一个可证明边界清楚的 edge command harness。

最终交付形态：

```text
MiniCPM real eval report
Mock release eval gate
Home Assistant demo gateway boundary
MQTT adapter with dry-run and guarded publish
MIoT/Xiaomi adapter contract and gated implementation path
Matter controller adapter contract and gated implementation path
explicit real execution mode
CI release gate
security / contribution / release governance
```

最终 README 允许宣传：

```text
EdgeHome Harness is a Rust safety harness for MiniCPM-powered edge home command agents.
MiniCPM proposes backend-neutral command candidates.
Rust validates, normalizes, resolves, gates, and plans.
Adapters translate verified plans into backend payloads.
Mock, Home Assistant demo, and MQTT adapter paths are covered by tests.
MIoT/Xiaomi and Matter require configured controller/device profiles before being claimed.
Real execution is explicit opt-in, not default.
```

最终 README 仍然不能宣传：

```text
生产级替代 Home Assistant
生产级替代米家
所有小米设备都支持
Matter full controller 已完成
默认真实执行
1B 模型可自主控制设备
mock eval 证明自然语言理解能力
```

---

## 3. 里程碑 M1：真实 MiniCPM/Ollama Eval Report

目标：

```text
把 mock release gate 和真实 MiniCPM/Ollama 表现分开。
mock gate 证明 harness regression。
real MiniCPM report 证明当前模型路径的实际输出稳定性。
```

### 3.1 要做的文件

新增或完善：

```text
docs/real-minicpm-eval-report.md
scripts/run-real-minicpm-eval.ps1
configs/eval_mode.yaml
README.md 中增加 Real MiniCPM Evaluation 链接
CHANGELOG.md 记录
```

### 3.2 执行步骤

1. 确认 Ollama 可用：

```powershell
ollama --version
ollama pull openbmb/minicpm5:1b
ollama list
```

2. 跑真实模型 eval：

```powershell
cargo run -q -p edgehome-cli -- --profile eval_mode --db-path "$env:TEMP\edgehome-real-minicpm.sqlite" eval cases\zh-home.yaml --ollama
```

3. 单独保存真实模型指标：

```text
model name
ollama version
hardware
OS
case count
category count
pass_rate
schema_valid_rate
dead_loop_rate
retry_rate
fallback_rate
latency_avg_ms
latency_p95_ms
false_allow_rate
fail_closed_rate
top failure categories
representative malformed outputs
```

4. 报告必须明确：

```text
This is real MiniCPM/Ollama model-path evaluation.
It is separate from the deterministic mock release gate.
It is not a broad natural-language understanding benchmark.
```

### 3.3 验收指标

最低可发布标准：

```text
真实模型 eval 可复现运行
report 中包含环境、命令、指标、失败样例
schema_valid_rate 被单独记录
false_allow_rate 必须为 0 才能公开展示
fail_closed_rate 必须为 1.0 才能公开展示
如果 pass_rate 不好看，也如实写，不粉饰
```

完成后提交：

```powershell
git add docs/real-minicpm-eval-report.md scripts/run-real-minicpm-eval.ps1 README.md CHANGELOG.md
git commit -m "eval: add real MiniCPM report workflow"
git push origin main
```

---

## 4. 里程碑 M2：Home Assistant Demo Config + Golden Payload 完整化

目标：

```text
把 Home Assistant 从“代码里有 demo adapter”完善成“外部读者能看懂、能 dry-run、能验证 payload、能安全接入”的 demo gateway boundary。
```

### 4.1 要做的文件

完善：

```text
configs/home_assistant.yaml.example
configs/devices.home_assistant.example.yaml
docs/home-assistant-demo.md
docs/backend-adapter-contract.md
docs/deployment-modes.md
README.md
```

新增：

```text
docs/home-assistant-golden-payloads.md
```

### 4.2 代码任务

检查并补齐：

```text
HomeAssistantConfig:
  base_url
  token_env
  token_file
  request_timeout_ms
  execute_enabled = false by default

HomeAssistantExecutor:
  dry_run does not require token
  execute requires token
  execute disabled by default
  entity_id validation
  missing route fail closed
  unsupported action fail closed
  post-state fetch path documented
```

### 4.3 Golden Payload

至少展示这些 payload：

```text
light.turn_on
light.turn_off
light.set_brightness
climate.turn_on
climate.turn_off
climate.set_temperature
climate.set_hvac_mode
```

每个 payload 必须说明：

```text
input Chinese command
candidate JSON
internal command
ExecutionPlan
Home Assistant service
service_path
payload
why entity_id is registry-owned
```

### 4.4 验收指标

```text
cargo test -p edgehome-executor home_assistant
golden payload tests pass
no token in dry-run payload
invalid entity_id rejected
missing route fails closed
execute disabled by default
docs show exact commands and expected output
```

完成后提交：

```powershell
git add configs docs README.md CHANGELOG.md crates/edgehome-executor/src
git commit -m "docs: expand Home Assistant demo boundary"
git push origin main
```

---

## 5. 里程碑 M3：最小可用 MQTT Adapter

目标：

```text
把 MQTT 从 future target 推进到 implemented adapter path。
先完成 dry-run payload 和 fail-closed tests，再做 explicit opt-in publish。
```

注意：

```text
MQTT 是 publish/subscribe transport，不是统一智能家居 payload 标准。
topic 和 payload 必须由 adapter config 提供。
MiniCPM 不能输出 topic。
```

### 5.1 Phase A：MQTT Dry-run Adapter

代码任务：

```text
crates/edgehome-executor/src/mqtt.rs
MqttConfig
MqttRoute
MqttAdapter
MqttPublishPlan
load_from_path
route lookup by device_id + action
template params.brightness / params.temperature / params.mode
validate topic
dry_run_payload
```

配置任务：

```text
configs/adapters/mqtt.example.yaml
configs/devices.mqtt.example.yaml
```

测试任务：

```text
MQTT light turn_on golden payload
MQTT light turn_off golden payload
MQTT brightness golden payload
missing route fails closed
invalid topic fails closed
payload does not contain secrets
model-generated topic ignored
```

README 状态更新：

```text
MQTT | Dry-run adapter implemented; guarded publish optional
```

### 5.2 Phase B：MQTT Guarded Publish

代码任务：

```text
MqttExecutor
execute_enabled = false by default
broker URL from env/config
username/password from env/file
QoS config
retain config
publish timeout
no secret in debug/trace
```

建议依赖：

```text
rumqttc
```

真实 broker 测试必须是 opt-in：

```powershell
$env:EDGEHOME_MQTT_BROKER_URL = "mqtt://127.0.0.1:1883"
$env:EDGEHOME_MQTT_EXECUTE_TESTS = "1"
cargo test -p edgehome-executor mqtt -- --ignored
```

### 5.3 验收指标

```text
dry-run MQTT payload tests pass
missing route fail closed
invalid topic fail closed
real publish disabled by default
ignored integration test can publish to test broker
README no longer says MQTT is only future target after Phase A is merged
docs explain MQTT is app-defined topic/payload mapping
```

完成后提交：

```powershell
git add crates/edgehome-executor/src configs docs README.md CHANGELOG.md
git commit -m "executor: add MQTT dry-run adapter"
git push origin main
```

如果 Phase B 完成：

```powershell
git add crates Cargo.toml Cargo.lock docs README.md CHANGELOG.md
git commit -m "executor: add guarded MQTT publish"
git push origin main
```

---

## 6. 里程碑 M4：MIoT / Xiaomi Adapter

目标：

```text
不要伪造“小米全支持”。
先做 config-driven MIoT adapter contract，再接真实测试设备。
只有有真实设备、真实 spec mapping、真实执行日志后，才能宣传 support。
```

### 6.1 必须先做的调查

需要确认：

```text
目标设备型号
局域网控制方式
是否需要 token
是否需要云账号
是否走 MIoT spec
did / siid / piid / aiid 来源
属性读写还是 action 调用
错误码和状态回读方式
```

不能做：

```text
模型输出 did / siid / piid / aiid
把网上某个设备 spec 当通用小米支持
没有设备就宣传真实支持
```

### 6.2 推荐实现路径

为了商用安全，MIoT adapter 分两层：

```text
EdgeHome Harness:
  validates command
  resolves device
  gates policy
  creates MIoT adapter payload

MIoT bridge/controller:
  owns token / local protocol / cloud protocol
  executes property/action call
  returns state / error
```

这样 Rust Harness 不直接硬编码所有小米设备协议。

### 6.3 代码任务

```text
crates/edgehome-executor/src/miot.rs
MiotConfig
MiotRoute
MiotAdapter
MiotActionPayload
MiotPropertyPayload
load_from_path
route lookup by device_id + action
did_env resolution only at execute layer
dry-run payload without secret values
missing did_env fail closed at execution
missing route fail closed
invalid siid/piid/aiid fail closed
```

### 6.4 测试任务

```text
MIoT action golden payload
MIoT set_properties golden payload
missing route fails closed
missing did_env fails closed for execution
payload does not contain token
model-generated MIoT IDs ignored/rejected by input guard
```

真实设备测试必须放在 ignored tests 或单独脚本：

```powershell
$env:EDGEHOME_MIOT_EXECUTE_TESTS = "1"
$env:EDGEHOME_MIOT_BEDROOM_AC_DID = "..."
$env:EDGEHOME_MIOT_TOKEN = "..."
cargo test -p edgehome-executor miot -- --ignored
```

### 6.5 README 允许更新的条件

只有满足这些条件，才从 future target 改口：

```text
至少一个真实 Xiaomi/MIoT 设备 profile
golden dry-run payload tests
fail-closed tests
ignored real-device test script
docs/miot-demo.md
明确写支持范围，不写“支持小米全家桶”
```

完成后提交：

```powershell
git add crates configs docs README.md CHANGELOG.md
git commit -m "executor: add MIoT adapter contract"
git push origin main
```

真实设备验证完成后另一个 commit：

```powershell
git add docs/miot-demo.md docs/real-device-validation.md README.md CHANGELOG.md
git commit -m "docs: add MIoT real-device validation notes"
git push origin main
```

---

## 7. 里程碑 M5：Matter Controller Adapter

目标：

```text
不要把 Matter 写成 JSON payload。
Matter adapter 必须对接 controller，或者对接外部 Matter bridge。
```

### 7.1 推荐实现路径

优先路径：

```text
EdgeHome Harness -> Matter controller bridge HTTP/gRPC/CLI -> Matter fabric
```

不要一开始就直接在本项目里实现完整 Matter controller。那会把项目范围扩大成另一个工程。

### 7.2 Matter Route Model

Matter route 必须来自配置：

```text
device_id
node_id
endpoint_id
cluster_id
command_id
attribute_id
value mapping
```

模型不能输出这些字段。

### 7.3 代码任务

```text
crates/edgehome-executor/src/matter.rs
MatterConfig
MatterRoute
MatterAdapter
MatterBridgeRequest
route lookup by device_id + action
dry-run bridge request payload
execute_enabled = false by default
bridge URL from config/env
missing route fail closed
invalid node/endpoint/cluster fail closed
```

### 7.4 测试任务

```text
Matter on/off bridge request golden payload
Matter brightness bridge request golden payload
missing route fails closed
invalid route ID fails closed
real bridge execution disabled by default
model-generated Matter IDs ignored/rejected
```

### 7.5 README 允许更新的条件

只有满足这些条件才能改口：

```text
Matter bridge dry-run adapter implemented
golden payload tests
fail-closed tests
docs/matter-bridge-demo.md
explicitly says controller/bridge required
does not claim full Matter controller support unless actually implemented
```

完成后提交：

```powershell
git add crates configs docs README.md CHANGELOG.md
git commit -m "executor: add Matter bridge adapter contract"
git push origin main
```

---

## 8. 里程碑 M6：商用级真实执行模式

目标：

```text
让真实执行可用，但不能默认开启。
```

### 8.1 Execution Mode

增加显式执行模式：

```text
DryRunOnly        默认
LocalConfirmed    本地确认后可执行
AutomationGuarded 自动化场景，但只允许 low-risk allowlist
EmergencyStop     全部真实执行关闭
```

配置建议：

```yaml
execution:
  mode: dry_run_only
  require_confirmation_for:
    - medium
    - high
  deny_risk:
    - blocked
  rate_limit:
    per_device_cooldown_ms: 1000
  idempotency_window_ms: 5000
  audit_required: true
  post_state_verification: true
```

### 8.2 CLI 行为

默认：

```powershell
dry-run -> only plans
```

真实执行必须显式：

```powershell
cargo run -q -p edgehome-cli -- execute --config private/execution.yaml --confirm <trace_id>
```

禁止：

```text
dry-run 命令自动执行
README 示例默认执行真实设备
没有 trace_id 的 execute
没有 confirm 的 medium/high risk execute
```

### 8.3 Audit / Trace

真实执行必须记录：

```text
trace_id
raw user input hash or redacted input
candidate JSON
normalized command
device_id
backend kind
dry-run payload hash
policy decision
confirmation evidence
backend response summary
post-state verification result
timestamp
```

### 8.4 验收指标

```text
execute disabled by default tests
low-risk confirmed execution tests
medium/high risk without confirmation rejected
blocked risk rejected
duplicate execution rejected
rate limit rejected
audit event recorded
post-state failure rejects transaction
secrets redacted from debug/log/trace
```

完成后提交：

```powershell
git add crates configs docs README.md CHANGELOG.md
git commit -m "executor: harden explicit execution mode"
git push origin main
```

---

## 9. 里程碑 M7：完整生产级 Home Assistant Gateway Boundary

目标：

```text
不是替代 Home Assistant。
而是让 EdgeHome Harness 可以作为 HA 前面的 safety harness gateway。
```

### 9.1 生产级 HA gateway 必须具备

```text
config-driven registry
route validation at startup
token isolation
dry-run preview
explicit execution mode
policy/risk confirmation
rate limit
idempotency
state fetch before execution
service call execution
state fetch after execution
post-state verification
audit log
replayable traces
clear failure modes
```

### 9.2 不做的事

```text
不替代 HA device integration
不管理 HA add-ons
不绕过 HA 权限
不把 HA token 放进 model prompt
不把 HA entity_id 交给 MiniCPM 生成
```

### 9.3 验收指标

```text
HA dry-run does not require token
HA execute requires token
HA execute disabled by default
invalid entity_id rejected
unsupported action rejected
missing route rejected
non-2xx response reported
state parse failure reported
post-state verification failure rejected
all tests pass
docs include private config template
```

完成后提交：

```powershell
git add crates docs configs README.md CHANGELOG.md
git commit -m "executor: harden Home Assistant gateway boundary"
git push origin main
```

---

## 10. 里程碑 M8：最终发布门

最终发布前必须全部通过：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-release-gate.sqlite" eval cases\zh-home.yaml --gate
git diff --check
```

额外检查：

```text
README backend matrix 与代码一致
CHANGELOG 完整
SECURITY.md 完整
CONTRIBUTING.md 完整
docs/roadmap.md 更新
docs/release-checklist.md 更新
没有真实 token
没有真实私有 URL
没有误称 MIoT/Matter 全量支持
没有默认真实执行示例
```

最终验收表：

| Area | Required final state |
| --- | --- |
| MiniCPM/Ollama | Real eval report exists and is separate from mock gate |
| Mock gate | 100+ cases, 12+ categories, `gate.passed = true` |
| Home Assistant | Demo gateway boundary with explicit execution mode |
| MQTT | Dry-run adapter implemented; guarded publish optional and tested |
| MIoT/Xiaomi | Config-driven adapter contract; support scope only claimed after real device validation |
| Matter | Bridge/controller adapter contract; no claim of full controller unless implemented |
| Execution | Explicit opt-in; default dry-run only |
| Safety | fail closed, rate limit, idempotency, audit, post-state verification |
| Docs | README, roadmap, release checklist, adapter docs, eval report all aligned |
| CI | fmt, clippy, tests, eval gate |

最终 release note 必须写：

```text
Implemented
Evidence
Known limitations
Backends supported today
Backends requiring external controller/device validation
Execution safety model
```

---

## 11. 当前下一步

从当前仓库状态看，最合理的执行顺序是：

```text
Step 1: commit this updated plan.md
Step 2: commit MQTT dry-run adapter
Step 3: expand Home Assistant golden payload docs
Step 4: add real MiniCPM/Ollama eval report workflow
Step 5: design MIoT adapter contract
Step 6: design Matter bridge adapter contract
Step 7: add MQTT guarded real publish behind explicit execution mode
Step 8: harden explicit execution mode
Step 9: final docs and release gate
```

不要先碰 MIoT/Matter 真实执行。没有真实设备、controller 和 secret 管理之前，先写真实执行很容易变成假的“支持”。

当前最小可交付 commit：

```powershell
git add plan.md crates/edgehome-executor/src configs docs README.md CHANGELOG.md SECURITY.md
git commit -m "executor: add MQTT dry-run adapter"
git push origin main
```
