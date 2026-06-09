# EdgeHome Harness 唯一实施计划

本文件是 EdgeHome Harness 后续实现的唯一规划指标。

执行时以本文件为准：

```text
README.md = 项目愿景、架构叙事、面试解释
PROJECT_DRAFT_LEGACY.md = 历史草稿和素材库
plan.md = 唯一实施顺序、验收标准、不得跳过的工程路线
```

任何后续 `/goal` 模式、长任务执行、代码实现、重构、文档更新，都必须优先遵守本计划。

如果实现过程中发现本计划不合理，必须先修改 `plan.md`，再继续写代码。

不能一边偏离计划，一边继续实现。

## 0. 总目标

构建一个面向 1B 端侧小模型的 Rust Agent Harness。

产品形态是智能家居本地控制中台。

更直接地说：

```text
这是一个 Rust-first 的端侧 Agent Harness 项目，
目标是在 2GB RAM 级别设备上通过 low_memory profile 可部署运行。
```

核心技术目标：

```text
MiniCPM5-1B + Ollama structured outputs
Rust Harness Core
Evidence-Gated Command Memory
Device Registry / Capability Model
Policy Gate / TransitionGate
Dry-run / Executor Router
Audit / Replay / Eval
2GB RAM low_memory profile
```

核心业务目标：

```text
用户用中文说智能家居 / IoT 指令
模型只输出受限 JSON 候选
Harness 把候选变成可验证命令
Harness 决定 allow / require_confirmation / deny
系统可以 dry-run、审计、回放、评测
低内存设备上可降级运行
```

第一版主模型：

```text
MiniCPM5-1B
```

第一版主运行时：

```text
Ollama structured outputs
```

第一版主后端：

```text
MockExecutor
```

第一版真实设备 demo 后端：

```text
HomeAssistantExecutor
```

后续对比模型：

```text
Qwen3.5-0.8B
```

注意：

```text
Qwen3.5-0.8B 不进入 V1 主链路。
它只作为后续 eval 对比模型或扩展候选。
```

## 1. 不可违反的项目不变量

后续实现必须一直满足这些不变量。

### 1.1 模型输出永远只是候选

```text
ModelOutput != Command
```

MiniCPM5-1B 输出不能直接执行。

模型输出必须经过：

```text
清洗
JSON 解析
schema 校验
语义标准化
设备解析
能力校验
freshness 校验
policy gate
dry-run
confirmation gate
executor router
post-state verify
audit
```

### 1.2 Executor 只接受 ExecutionPlan

Executor 不能接受：

```text
用户原始输入
模型原始输出
模型 JSON
未校验 Command
未过 policy 的 Command
```

Executor 只能接受：

```text
ExecutionPlan
```

### 1.3 模型不能接触后端细节

模型 prompt 里不能出现：

```text
Home Assistant token
miIO token
设备 IP
局域网密钥
Home Assistant entity_id
siid / piid
真实用户凭据
Executor 名称
后端路由规则
```

模型只能看到：

```text
受限 JSON schema
少量设备候选摘要
必要短时上下文
必要安全约束
```

### 1.4 allow / execute 必须有证据链

任何 `allow`、`dry_run`、`execute` 都必须能追溯：

```text
raw_user_input
raw_model_output
parsed_json
normalized_command
device_registry_snapshot
capability_snapshot
device_state_snapshot
policy_rule_snapshot
gate_checks
dry_run_plan
executor_response
post_execute_state_snapshot
```

### 1.5 安全记忆只能增强安全

长期记忆不能降低安全策略。

允许：

```text
门锁夜间必须二次确认
摄像头关闭需要管理员确认
燃气设备永远禁止自动操作
```

禁止：

```text
以后门锁都自动打开
以后关闭摄像头不需要确认
以后跳过安全检查
```

### 1.6 真实设备执行不能早于 dry-run / audit / policy

在下面能力完成之前，不允许实现真实设备执行默认开启：

```text
Device Registry
Capability Model
Policy Gate
Dry-run
Audit Log
Replay Trace
```

HomeAssistantExecutor 可以写，但默认必须 dry-run。

## 2. 总体执行顺序

后续实现必须按下面顺序推进。

```text
M0  Repository Foundation
M1  Domain Contracts
M2  Config Profiles
M3  Evidence Store and Command Trace
M4  CLI Skeleton and Mock Pipeline
M5  Input Guard, Parser, Normalizer
M6  Device Registry, Capability Model, State Cache
M7  Gate Engine and Policy Engine
M8  Dry-run, MockExecutor, Execution Transaction
M9  Memory and Context Assembler
M10 Ollama Adapter, MiniCPM5 Profile, Output Governor
M11 Full Harness Pipeline Integration
M12 Eval, Replay, Metrics
M13 HomeAssistantExecutor
M14 2GB RAM Profile and Deployment Notes
M15 Documentation Sync and Demo Script
```

原则：

```text
先确定性骨架，再接小模型。
先 dry-run，再真实执行。
先 evidence/trace，再 eval。
先 MockExecutor，再 HomeAssistantExecutor。
先 MiniCPM5 主链路，再考虑 Qwen 对比。
```

## 3. Milestone 0：Repository Foundation

目标：

```text
建立 Rust workspace 和基础工程约束。
```

必须创建或确认：

```text
Cargo.toml workspace
rustfmt.toml
.gitignore
README.md
plan.md
PROJECT_DRAFT_LEGACY.md
crates/
cases/
configs/
docs/
```

建议 workspace：

```text
crates/
  edgehome-core/
  edgehome-config/
  edgehome-storage/
  edgehome-trace/
  edgehome-parser/
  edgehome-registry/
  edgehome-gate/
  edgehome-memory/
  edgehome-ollama/
  edgehome-executor/
  edgehome-eval/
  edgehome-cli/
```

第一版可以不创建 server crate。

不要一开始做 Web UI。

建议依赖：

```text
serde
serde_json
serde_yaml 或 toml
schemars
thiserror
anyhow 仅 CLI 层可用
clap
tracing
tracing-subscriber
chrono 或 time
uuid
rusqlite
reqwest
tokio
```

验收标准：

```text
cargo check 通过
cargo fmt --check 通过
edgehome-cli crate 可以构建
README.md 和 plan.md 保持存在
```

禁止事项：

```text
不要接 Ollama
不要接 Home Assistant
不要做真实设备执行
```

## 4. Milestone 1：Domain Contracts

目标：

```text
定义所有模块共享的核心类型。
```

位置建议：

```text
crates/edgehome-core/
```

必须定义：

```text
UserInput
ModelCandidate
ModelOutputSchemaVersion
CommandSchemaVersion
NormalizedCommand
Intent
Room
DeviceType
DeviceId
Action
CommandParams
RiskLevel
PolicyDecision
ExecutionPlan
DryRunPlan
ExecutionResult
HarnessError
```

第一版 Intent：

```text
control_device
query_status
create_rule
update_memory
unknown
```

第一版 Action：

```text
turn_on
turn_off
set_brightness
increase_brightness
decrease_brightness
set_temperature
set_mode
open
close
lock
unlock
unknown
```

第一版 DeviceType：

```text
light
air_conditioner
curtain
switch
camera
lock
sensor
gas_device
unknown
```

必须有 schema version：

```text
model_output.v1
command.v1
device_registry.v1
policy.v1
memory.v1
```

验收标准：

```text
所有核心类型支持 Serialize / Deserialize
所有核心类型有单元测试
可以生成或维护给 Ollama format 使用的 JSON schema
unknown 默认 fail closed
```

测试要求：

```text
cargo test -p edgehome-core
```

禁止事项：

```text
不要在 core 里放 HTTP 调用
不要在 core 里放 SQLite 具体实现
不要让 core 依赖 ollama/executor
```

## 5. Milestone 2：Config Profiles

目标：

```text
把模型参数、内存策略、安全策略、执行器配置全部 profile 化。
```

位置建议：

```text
crates/edgehome-config/
configs/
```

必须支持 profile：

```text
strict_mode
normal_mode
low_memory_mode
eval_mode
demo_mode
```

配置字段：

```text
model_name
ollama_base_url
temperature
top_p
top_k
repeat_penalty
num_ctx
num_predict
timeout_ms
retry_count
memory_enabled
max_short_memory_turns
max_context_chars
executor_backend
dangerous_action_policy
audit_enabled
trace_enabled
```

低内存 profile 默认：

```text
num_ctx <= 1024
num_predict <= 128
max_short_memory_turns <= 3
max_context_chars <= 500
retry_count <= 1
executor_backend = mock
```

验收标准：

```text
可以加载 configs/low_memory.yaml
可以通过 CLI 指定 --profile low_memory
配置缺失时有明确错误
危险默认值必须偏保守
```

测试要求：

```text
cargo test -p edgehome-config
```

禁止事项：

```text
不要把模型参数硬编码在 adapter 里
不要把 executor backend 写死
```

## 6. Milestone 3：Evidence Store and Command Trace

目标：

```text
先建立 evidence / trace / audit 骨架。
```

原因：

```text
后续所有 pipeline 都必须可追溯。
如果晚做 evidence，后面会大量返工。
```

位置建议：

```text
crates/edgehome-storage/
crates/edgehome-trace/
```

必须定义：

```text
EvidenceId
EvidenceRef
EvidenceKind
SourceSystem
Freshness
CommandTrace
CommandStep
StepStatus
GateCheck
AuditEvent
```

EvidenceKind 第一版：

```text
raw_user_input
raw_model_output
parsed_json
normalized_command
device_registry_snapshot
capability_snapshot
device_state_snapshot
policy_rule_snapshot
user_confirmation
dry_run_plan
executor_request
executor_response
post_execute_state_snapshot
eval_case
eval_result
memory_write_request
memory_item
```

SQLite 表第一版：

```text
evidence_refs
command_traces
command_steps
step_evidence_refs
gate_checks
audit_log
```

必须实现：

```text
EvidenceStore::record
EvidenceStore::read
EvidenceStore::freshness
TraceStore::start_trace
TraceStore::append_step
TraceStore::append_gate_check
AuditSink::append
```

验收标准：

```text
一次 mock dry-run 可以生成 trace_id
trace_id 能查到 raw_user_input_ref
每个 step 能关联 evidence_refs
gate check 能记录 accepted / rejected / reason
```

测试要求：

```text
cargo test -p edgehome-storage
cargo test -p edgehome-trace
```

禁止事项：

```text
不要把 Mermaid 当运行时状态源
不要让 LLM 写 evidence
不要把 raw secrets 写入 evidence
```

## 7. Milestone 4：CLI Skeleton and Mock Pipeline

目标：

```text
在没有 Ollama 的情况下跑通确定性 pipeline。
```

位置建议：

```text
crates/edgehome-cli/
crates/edgehome-core/
```

必须实现命令：

```bash
edgehome parse --mock "把客厅灯关掉"
edgehome dry-run --mock "晚上十点后把走廊灯调到30%"
edgehome trace show <trace_id>
```

MockModel 行为：

```text
输入固定 case
返回固定 ModelCandidate JSON
用于测试 pipeline，不依赖模型
```

必须打通：

```text
raw_user_input evidence
mock raw_model_output evidence
parsed_json evidence
normalized_command evidence
command_trace
audit_log
```

验收标准：

```text
edgehome dry-run --mock 能输出 trace_id
edgehome trace show <trace_id> 能看到步骤链
mock pipeline 不需要 Ollama
```

测试要求：

```text
cargo test
cargo run -p edgehome-cli -- dry-run --mock "把客厅灯关掉"
```

禁止事项：

```text
不要为了 demo 跳过 trace
不要在 mock pipeline 中直接执行设备
```

## 8. Milestone 5：Input Guard, Parser, Normalizer

目标：

```text
完成模型输出清洗、JSON 解析、schema 校验、语义标准化。
```

位置建议：

```text
crates/edgehome-parser/
```

必须实现：

```text
InputGuard
RulePreParser
OutputCleaner
JsonExtractor
SchemaValidator
SemanticNormalizer
TimeNormalizer
NumberNormalizer
```

InputGuard 处理：

```text
输入长度限制
非法控制字符
prompt injection 标记
危险直连语句标记
```

OutputCleaner 处理：

```text
<think>...</think>
markdown code fence
JSON 前后多余文本
找不到 JSON fail closed
JSON 闭合后多余内容截断
```

SemanticNormalizer 处理：

```text
客厅 -> living_room
卧室 -> bedroom
走廊 -> hallway
关掉 -> turn_off
打开 -> turn_on
调到 30% -> set_brightness brightness=30
26 度 -> temperature=26
晚上十点 -> 22:00
再暗一点 -> 依赖短时记忆，先标记为 relative_command
```

验收标准：

```text
非法模型输出不会 panic
非法模型输出不会进入 executor
中文常见指令可以标准化
所有 parser 失败都能写入 trace
```

测试用例必须包括：

```text
纯 JSON
带 think block
带 markdown fence
JSON 后有多余文本
无 JSON
重复 key
亮度越界
时间表达
相对指代
```

测试要求：

```text
cargo test -p edgehome-parser
```

## 9. Milestone 6：Device Registry, Capability Model, State Cache

目标：

```text
建立智能家居设备抽象，不让模型知道真实后端。
```

位置建议：

```text
crates/edgehome-registry/
configs/devices.yaml
```

必须实现：

```text
DeviceRegistry
DeviceAliasResolver
CapabilityResolver
DeviceStateProvider
StateCache
```

Device Registry 示例：

```yaml
devices:
  - device_id: living_room_main_light
    aliases: ["客厅灯", "客厅主灯", "屋里大灯"]
    room: living_room
    device_type: light
    backend: mock
    backend_entity_id: mock.light.living_room_main
    risk_level: low
```

Capability Model 示例：

```yaml
capabilities:
  light:
    - action: turn_on
    - action: turn_off
    - action: set_brightness
      min: 0
      max: 100
      unit: percent
```

State Cache 第一版：

```text
MockStateProvider
state_json
observed_at
stale_after_sec
expired_after_sec
```

验收标准：

```text
客厅灯 -> living_room_main_light
不存在设备 -> rejected
light + set_temperature -> rejected
brightness=150 -> rejected
state freshness 可以判断 fresh/stale/expired/unknown
```

测试要求：

```text
cargo test -p edgehome-registry
```

禁止事项：

```text
不要把 Home Assistant token 放进 registry
不要把 entity_id 注入模型 prompt
不要让模型选择 backend
```

## 10. Milestone 7：Gate Engine and Policy Engine

目标：

```text
建立真正的 Harness 安全边界。
```

位置建议：

```text
crates/edgehome-gate/
crates/edgehome-policy/
```

必须实现 gate：

```text
SchemaGate
DeviceResolvedGate
CapabilityGate
FreshnessGate
PolicyGate
ConfirmationGate
DryRunGate
ExecutionGate
MemoryWriteGate
```

PolicyDecision：

```text
allow
require_confirmation
deny
```

风险模型：

```text
read -> allow
low -> allow / dry-run
medium -> audit / require_confirmation
high -> require_confirmation
blocked -> deny
unknown -> deny
```

TransitionGate 必须阻止：

```text
execute 越过 policy_decision
execute 越过 dry_run
high-risk execute 越过 confirmation
verify_state 在 execute 前发生
memory_write 直接相信 LLM 输出
policy_allowed 使用 expired policy snapshot
state_based_action 使用 expired device state
```

验收标准：

```text
打开门锁 -> require_confirmation
关闭所有摄像头 -> require_confirmation 或 deny，按策略配置
关闭燃气报警器 -> deny
unknown device -> deny
unsupported capability -> deny
每次 gate 都写 gate_checks
```

测试要求：

```text
cargo test -p edgehome-gate
cargo test -p edgehome-policy
```

禁止事项：

```text
不要让模型决定 policy
不要让 executor 绕过 gate
不要让 unknown 默认 allow
```

## 11. Milestone 8：Dry-run, MockExecutor, Execution Transaction

目标：

```text
实现不碰真实设备的安全执行计划。
```

位置建议：

```text
crates/edgehome-executor/
```

必须实现：

```text
Executor trait
MockExecutor
DryRunPlanner
ExecutionTransaction
IdempotencyChecker
RateLimiter
PostStateVerifier mock
```

ExecutionTransaction：

```text
validate
  -> policy
  -> dry-run
  -> optional confirmation
  -> idempotency check
  -> execute
  -> post-state verify
  -> audit
```

第一版默认：

```text
execute disabled
dry-run enabled
```

验收标准：

```text
edgehome dry-run --mock "晚上十点后把走廊灯调到30%" 输出 ExecutionPlan
ExecutionPlan 包含 trace_id
高风险动作没有 confirmation 不能 execute
重复命令可以被 idempotency checker 识别
```

测试要求：

```text
cargo test -p edgehome-executor
```

禁止事项：

```text
不要默认真实执行
不要隐藏 executor failure
不要执行失败还返回 success
```

## 12. Milestone 9：Memory and Context Assembler

目标：

```text
实现轻量化本地记忆，并把记忆编译成短 prompt 上下文。
```

位置建议：

```text
crates/edgehome-memory/
crates/edgehome-core/
```

必须实现：

```text
ShortSessionMemory
LongTermPreferenceStore
SafetyMemory
MemoryWriteGate
ContextAssembler
```

短时记忆：

```text
last_device_id
last_room
last_device_type
last_action
last_value
last_trace_id
expires_at
```

长期记忆：

```text
device_alias
room_alias
user_preference
scene_default
safety_rule
```

ContextAssembler 输出限制：

```text
max_short_memory_turns <= profile
max_context_chars <= profile
最多注入少量候选设备
最多注入少量长期偏好
不注入 raw refs 原文
不注入 secrets
```

必须支持场景：

```text
用户：把客厅灯调到70%
用户：再暗一点
系统：解析到 living_room_main_light + decrease_brightness
```

验收标准：

```text
短时记忆能解析“刚才那个灯”
长期记忆只能由明确用户表达或确认写入
安全记忆只能增强安全
低内存 profile 可以禁用长期记忆注入
```

测试要求：

```text
cargo test -p edgehome-memory
```

禁止事项：

```text
不要让 LLM 自动写长期记忆
不要把完整聊天历史塞进 prompt
不要把 KV cache 当业务记忆
```

## 13. Milestone 10：Ollama Adapter, MiniCPM5 Profile, Output Governor

目标：

```text
接入 MiniCPM5-1B，但仍然保持 Harness 可控。
```

位置建议：

```text
crates/edgehome-ollama/
```

必须实现：

```text
OllamaClient
MiniCPM5 model profile
StructuredOutputRequest
StructuredOutputResponse
OutputGovernor
RetryPolicy
FallbackPolicy
ModelHealth
```

第一版只支持：

```text
Ollama /api/chat
MiniCPM5-1B
non-streaming
structured outputs
```

streaming 可以后续实现。

OutputGovernor 必须处理：

```text
timeout
max output bytes
max output chars
num_predict profile
invalid JSON retry
schema failed retry
fallback to enum-only
fallback to rule-only
model health circuit breaker
```

默认参数：

```text
temperature 0.0-0.2
top_p 0.8-0.95
top_k 20
repeat_penalty 1.2-1.3
num_ctx 1024
num_predict 80-160
timeout_ms profile controlled
retry_count <= 1 in low_memory
```

验收标准：

```text
edgehome parse "把客厅灯关掉" 可以调用 MiniCPM5
原始模型输出写入 evidence
模型 timeout 写入 trace
模型失败不会进入 executor
低内存 profile 生效
```

测试要求：

```text
cargo test -p edgehome-ollama
```

集成测试可以允许在 Ollama 不存在时自动 skip，但必须有清楚提示。

禁止事项：

```text
不要接多个模型平台
不要把 Qwen 加进主链路
不要绕过 OutputGovernor
```

## 14. Milestone 11：Full Harness Pipeline Integration

目标：

```text
把 mock pipeline、parser、registry、gate、memory、Ollama、dry-run 串成主链路。
```

主命令：

```bash
edgehome dry-run "晚上十点后把走廊灯调到30%"
```

必须输出：

```text
trace_id
normalized_command
policy_decision
dry_run_plan
evidence_refs
gate_checks
```

必须串联：

```text
InputGuard
RulePreParser
ContextAssembler
OllamaClient or MockModel
OutputGovernor
OutputCleaner
SchemaValidator
SemanticNormalizer
EvidenceStore
DeviceRegistry
CapabilityResolver
FreshnessGate
PolicyEngine
DryRunPlanner
AuditSink
MemoryUpdateGate
```

验收标准：

```text
成功 case 可以 dry-run
危险 case 可以 require_confirmation / deny
失败 case 可以 trace show
所有主路径都有 trace_id
所有 allow / deny 都有 gate_checks
```

必须通过 case：

```text
把客厅灯关掉
把卧室空调调到26度
晚上十点后把走廊灯调到30%
打开前门门锁
关闭所有摄像头
关闭燃气报警器
把刚才那个灯再调暗一点
```

测试要求：

```text
cargo test
cargo run -p edgehome-cli -- dry-run --mock "晚上十点后把走廊灯调到30%"
```

有 Ollama 时再运行：

```text
cargo run -p edgehome-cli -- dry-run "晚上十点后把走廊灯调到30%"
```

## 15. Milestone 12：Eval, Replay, Metrics

目标：

```text
证明 Harness 有用，而不是凭感觉说模型可用。
```

位置建议：

```text
crates/edgehome-eval/
cases/
reports/
```

必须实现：

```text
YAML case loader
EvalRunner
ReplayLoader
MetricsCalculator
MarkdownReport
JsonReport
```

初始 cases：

```text
intent / slot cases
policy cases
dangerous action cases
memory cases
invalid JSON cases
timeout / fallback cases
stale state cases
```

指标：

```text
valid JSON rate
schema pass rate
intent accuracy
slot accuracy
normalization success rate
policy correctness
dangerous action block rate
confirmation correctness
memory fallback success rate
timeout rate
dead-loop interruption rate
Evidence Source Coverage
False Execution Block Rate
Stale State Leakage Rate
Actionable Rejection Rate
Context Budget Efficiency
Audit Coverage
Replay Success Rate
latency p50 / p95
RSS peak 手动或脚本记录
```

命令：

```bash
edgehome eval cases/zh-home.yaml --profile eval_mode
edgehome replay <trace_id>
```

验收标准：

```text
eval 能跑完整 cases
报告输出 Markdown 和 JSON
每个 failed case 有 trace_id
replay 能复现至少 normalized_command / policy_decision / dry_run_plan
```

禁止事项：

```text
不要把 eval 放到最后才补
不要只看模型准确率，不看 gate 和 evidence coverage
```

## 16. Milestone 13：HomeAssistantExecutor

目标：

```text
在核心 Harness 稳定后，接入 Home Assistant 作为真实设备 demo 后端。
```

位置建议：

```text
crates/edgehome-executor/
configs/home_assistant.yaml.example
```

必须实现：

```text
HomeAssistantClient
HomeAssistantExecutor
HA state fetch
HA dry-run translation
HA service call translation
Secrets loader
```

第一版支持：

```text
light.turn_on
light.turn_off
switch.turn_on
switch.turn_off
climate.set_temperature
cover.open_cover
cover.close_cover
```

默认行为：

```text
dry-run only
execute 需要显式 --confirm
high risk 永远 confirmation
blocked 永远 deny
```

验收标准：

```text
不配置 HA token 时不会 panic
HA token 不进入 prompt
HA token 不进入普通 audit
Device Registry 可以映射 device_id -> entity_id
dry-run 能显示 HA service 和 payload
execute 只有 confirm 后才可调用
```

禁止事项：

```text
不要宣传所有米家设备都离线
不要把 Home Assistant 变成项目本体
不要让模型输出 entity_id
```

## 17. Milestone 14：2GB RAM Profile and Deployment Notes

目标：

```text
证明系统设计考虑端侧低内存约束。
```

必须完成：

```text
configs/low_memory.yaml
docs/2gb-profile.md
docs/model-parameters.md
docs/deployment-modes.md
```

需要记录：

```text
测试机器
OS
RAM
模型名
量化版本
num_ctx
num_predict
temperature
timeout
平均延迟
p95 延迟
RSS peak
失败模式
降级行为
```

部署模式：

```text
Mode A：2GB Edge Harness + HA on LAN
Mode B：4GB/8GB all-in-one
Mode C：2GB ultra-local + MiioLocalExecutor subset
```

验收标准：

```text
low_memory profile 文档完整
README 与 docs 不冲突
内存压力下会减少记忆注入
内存压力下会减少 num_ctx / num_predict 或进入 rule-only
```

## 18. Milestone 15：Documentation Sync and Demo Script

目标：

```text
让项目可以被面试官快速理解和复现。
```

必须维护：

```text
README.md
plan.md
PROJECT_DRAFT_LEGACY.md
docs/model-parameters.md
docs/2gb-profile.md
docs/home-assistant-demo.md
docs/eval-report-example.md
```

Demo 脚本：

```text
1. edgehome eval cases/zh-home.yaml
2. edgehome dry-run "晚上十点后把走廊灯调到30%"
3. edgehome replay <trace_id>
4. 展示 dangerous action 被 gate 拦截
5. 展示“刚才那个灯”被短时记忆解析
6. 展示 low_memory profile 限制上下文
```

README 必须持续表达：

```text
这是 1B 端侧小模型 Harness 项目
智能家居是产品形态和验证场景
Ollama 只保证 JSON 语法
Harness 保证业务安全、证据门控、运行时稳定
模型输出只是 candidate
Executor 只接受 ExecutionPlan
```

验收标准：

```text
README、plan、docs 没有核心冲突
plan.md 仍然是唯一实施路线
demo 命令能跑
eval report 能生成
```

## 19. 第一版完成定义

V1 完成必须同时满足：

```text
cargo fmt --check 通过
cargo test 通过
edgehome dry-run --mock "晚上十点后把走廊灯调到30%" 通过
edgehome eval cases/zh-home.yaml --profile eval_mode 通过
edgehome replay <trace_id> 通过
危险动作被正确拦截
每次 allow / deny 都有 gate_checks
每次 dry-run 都有 trace_id
README 和 plan 同步
```

第一版不要求：

```text
真实米家设备全离线控制
完整 Web UI
多模型平台
Qwen 主链路
向量数据库
开放式 RAG
自主 Agent loop
```

## 20. 后续扩展顺序

V1 之后再考虑：

```text
Qwen3.5-0.8B eval 对比
streaming output governor
llama.cpp runtime adapter
GBNF grammar support
Home Assistant capability sync
MiioLocalExecutor
MQTT Executor
Matter Executor
local web dashboard
systemd deployment
ARM cross compile
真实 2GB board benchmark
```

扩展原则：

```text
任何扩展都不能破坏 evidence / gate / trace 主线。
任何真实执行都不能绕过 ExecutionPlan。
任何模型扩展都不能绕过 OutputGovernor。
任何记忆扩展都不能绕过 MemoryWriteGate。
```

## 21. 后续 goal 模式执行规则

进入 `/goal` 或长任务实现时，必须遵守：

```text
1. 先读取 README.md 和 plan.md
2. 以 plan.md 当前 milestone 为准
3. 不跨 milestone 做功能
4. 每个 milestone 完成后运行对应测试
5. 测试失败不进入下一 milestone
6. 如果需要改变顺序，先修改 plan.md
7. 不为 demo 跳过 gate / trace / audit
8. 不在核心 Harness 稳定前做真实设备执行
9. 不因为模型输出正确就省略 validator
10. 不因为 JSON 合法就省略 policy
11. 经常提交留档，完成一个可验证 checkpoint 就提交一次
```

每个 milestone 的结束报告必须包含：

```text
完成了什么
修改了哪些文件
新增了哪些命令
跑了哪些测试
哪些验收标准通过
哪些风险仍然存在
下一步 milestone 是什么
本次提交 hash 是什么
```

## 22. 当前完成状态

V1 已按本计划从 Milestone 0 推进到 Milestone 15。

当前已完成：

```text
M0  Repository Foundation
M1  Domain Contracts
M2  Config Profiles
M3  Evidence Store and Command Trace
M4  CLI Skeleton and Mock Pipeline
M5  Input Guard, Parser, Normalizer
M6  Device Registry, Capability Model, State Cache
M7  Gate Engine and Policy Engine
M8  Dry-run, MockExecutor, Execution Transaction
M9  Memory and Context Assembler
M10 Ollama Adapter, MiniCPM5 Profile, Output Governor
M11 Full Harness Pipeline Integration
M12 Eval, Replay, Metrics
M13 HomeAssistantExecutor
M14 2GB RAM Profile and Deployment Notes
M15 Documentation Sync and Demo Script
```

V1 完成后的继续扩展必须遵守：

```text
plan.md 仍然是唯一实施路线
任何新 milestone 先写入 plan.md，再实现
不为 demo 跳过 evidence / gate / trace / audit
不让模型绕过 OutputGovernor
不让真实执行绕过 ExecutionPlan
不把 Home Assistant 变成项目本体
不宣传所有米家设备都纯离线
```
