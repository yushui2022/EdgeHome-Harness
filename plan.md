# EdgeHome Harness 对外发布加固执行计划

最后更新：2026-07-04

本文档是后续继续执行 EdgeHome Harness 的权威接手计划。目标不是复述聊天记录，而是在上下文压缩、换线程、换人接手或中断后，只要先读本文件，就能继续把项目做到一个可以对外宣传、但不过度吹嘘的状态。

核心判断：

```text
项目定位是成立的，但必须精准表达。

已实现主线：
MiniCPM/Mock candidate JSON
  -> Rust schema validation / normalization
  -> DeviceResolver
  -> GateEngine::verify
  -> GatedCommand
  -> DryRunPlanner::plan_gated
  -> Mock / Home Assistant dry-run payload

未实现能力：
真实 MIoT / Xiaomi adapter
真实 Matter controller adapter
真实 MQTT adapter
真实设备执行默认开启
真实 2GB ARM 长跑 benchmark
```

对外一句话必须保持：

```text
MiniCPM proposes. Rust decides. Adapters translate.
```

---

## 1. 恢复执行时先做什么

任何继续执行的人都先在仓库根目录运行：

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

不要回滚不认识的改动。先判断它属于哪个里程碑，再继续。

每个可提交里程碑完成前必须运行：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-release-gate.sqlite" eval cases\zh-home.yaml --gate
git diff --check
```

只有这些检查通过，才提交并推送：

```powershell
git add <changed-files>
git commit -m "<milestone commit message>"
git push origin main
```

提交节奏：

```text
一个独立能力点通过验证后就 commit。
不要把质量门修复、架构改动、README 改写、eval 扩容混在一个 commit。
每次 commit 后 push，避免长时间只存在本地。
```

---

## 2. 不变量

后续代码、README、docs、宣传页和 demo 都必须遵守这些不变量。

### 2.1 ModelOutput != Command

MiniCPM 输出永远只是候选 JSON，不是命令。

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

### 2.2 模型输出 schema 固定，后端映射可定制

不要把项目说成：

```text
让小模型按不同厂家要求输出任意 JSON。
```

正确说法：

```text
The model output contract is fixed and safe.
The device registry and backend adapter mappings are customizable.
```

中文口径：

```text
模型输出不定制，后端映射可定制。
```

### 2.3 设备真相在 Registry

模型可以提出：

```text
room
device_alias
device_type
action
params
```

模型不能发明最终设备 ID 或后端路由。最终目标必须由 `DeviceRegistry` 和 `DeviceResolver` 决定。

宣传口径：

```text
The model never decides device IDs.
Device truth lives in the registry.
```

### 2.4 未实现 backend 必须 fail closed

当前可以宣传：

```text
Mock implemented.
Home Assistant demo adapter implemented.
MIoT / Xiaomi, Matter, and MQTT are future adapter targets.
Unsupported backends fail closed.
```

当前不能宣传：

```text
Supports Xiaomi.
Supports MIoT.
Supports Matter.
Supports MQTT.
Production-ready smart-home gateway.
```

### 2.5 真实执行默认关闭

默认只允许：

```text
dry-run
mock payload
Home Assistant service-call payload demo
trace / replay / audit
eval / release gate
```

不能默认依赖：

```text
真实小米设备
真实 Home Assistant 实例
真实 Matter controller
真实 MQTT broker
局域网 token
云账号
```

---

## 3. 当前真实状态

### 3.1 已完成能力

工程结构：

```text
Rust workspace
edgehome-core
edgehome-parser
edgehome-registry
edgehome-gate
edgehome-executor
edgehome-memory
edgehome-ollama
edgehome-storage
edgehome-trace
edgehome-eval
edgehome-cli
```

模型与候选输出：

```text
MiniCPM5-1B low-memory profile
Ollama structured-output adapter
Mock model path for deterministic demo and eval
Output governor for invalid JSON, overlong output, repetition, retry and fallback
```

命令链路：

```text
ModelCandidate
NormalizedCommand
DeviceResolver
GateEvaluation
GatedCommand
ExecutionPlan
DryRunPlan
```

安全与审计：

```text
InputGuard
SchemaGate
DeviceResolvedGate
CapabilityGate
FreshnessGate
PolicyGate
ConfirmationGate
DryRunGate
ExecutionGate
MemoryWriteGate
Trace / Evidence / Audit / Replay
```

后端边界：

```text
MockAdapter implemented
HomeAssistantAdapter demo implemented
MiioLocal fails closed
Mqtt fails closed
Matter only appears as future target in docs
```

文档：

```text
README.md 已英文国际化
README.md 已加入 docs/assets/edgehome-harness-overview.jpg
docs/customization.md 已说明可定制边界
docs/command-pipeline-contract.md 已说明三层命令链路
docs/backend-adapter-contract.md 已说明 adapter 规则
configs/adapters/miot.example.yaml 是 future design only
configs/adapters/mqtt.example.yaml 是 future design only
```

### 3.2 当前对外可说

可以说：

```text
EdgeHome Harness is a Rust safety harness for MiniCPM-powered edge home command agents.
MiniCPM emits backend-neutral candidate JSON.
Rust validates, normalizes, resolves, gates, traces, and evaluates the command.
Backend adapters translate verified internal plans into backend-specific dry-run payloads.
Mock and Home Assistant demo payload paths are implemented.
Unsupported backend targets fail closed.
```

不可以说：

```text
The project controls Xiaomi devices.
The project supports Matter.
The project supports MQTT.
The project is production ready.
The project has been proven on a 2GB production board.
The model can directly output whatever JSON a vendor needs.
```

---

## 4. 已完成里程碑

### M1. 质量门修复

状态：Done

目的：

```text
对外发布前，基础 Rust 工程质量必须干净。
```

已完成：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
eval gate
git diff --check
```

代表提交：

```text
chore: make Rust quality gates pass
```

### M2. Backend adapter 显式化和 fail closed

状态：Done

前置逻辑：

```text
如果未实现的 backend 会落到 mock payload，外部读者会误以为 MIoT/MQTT 已经可用。
```

已完成：

```text
BackendAdapter trait
MockAdapter
HomeAssistantAdapter
BackendAdapterNotImplemented
MiioLocal fail closed
Mqtt fail closed
unsupported backend tests
```

代表提交：

```text
feat: make backend adapters explicit
```

### M3. 文档边界和定制化合同

状态：Done

前置逻辑：

```text
用户关心不同厂家 JSON 是否一致。答案是不一致。
因此项目不能让模型输出厂家 JSON，而要固定模型 schema，把差异放到 adapter。
```

已完成：

```text
docs/customization.md
docs/command-pipeline-contract.md
docs/backend-adapter-contract.md
README support matrix
MIoT/MQTT example config marked as future design only
```

代表提交：

```text
docs: clarify command and backend boundaries
docs: define customization contract
```

### M4. DeviceResolver 接管设备解析

状态：Done

前置逻辑：

```text
SemanticNormalizer 不应该硬编码 room + device_type + action -> device_id。
设备真相必须来自 registry 和受控 memory。
```

已完成：

```text
SemanticNormalizer no longer hardcodes device_id
DeviceRegistry::device_resolver()
DeviceRegistry::devices_in_room_by_type(...)
DeviceResolver
DeviceResolutionInput
DeviceResolution
DeviceResolutionSource
RegistryError for ambiguous/no-match/missing-slots
CLI resolve_device_target(...)
device_resolution trace step
registry alias tests
room/type unique match tests
zero match tests
ambiguous match tests
alias type mismatch tests
CLI test for registry resolution without memory items
```

代表提交：

```text
feat: resolve devices through registry
```

### M5. GatedCommand typed dry-run boundary

状态：Done in current milestone

前置逻辑：

```text
Layer 2 到 Layer 3 不能只靠 if 判断。
Dry-run planner 的主路径应该消费 gate 明确接受后的对象。
```

已完成：

```text
GateEvaluation::can_plan_dry_run()
GatedCommand
GateCommandDecision
GateEngine::verify(...)
DryRunPlanner::plan_gated(...)
ExecutorError::GateRejected
CLI dry-run path uses GateEngine::verify(...)
CLI dry-run path only plans from Accepted { gated_command }
gate tests for accepted/rejected decisions
executor tests for gated planning and fail-closed backends
```

本里程碑完成后推荐提交：

```powershell
git add crates/edgehome-gate crates/edgehome-executor crates/edgehome-cli plan.md
git commit -m "feat: type gate dry-run planning"
git push origin main
```

---

## 5. 后续里程碑

### M6. Eval cases 扩到 100 条

状态：Done in current milestone

为什么要做：

```text
M6 之前 eval baseline 只有 15 条，宣传时容易被质疑只是挑了几个样例。
100 条 eval case 的意义不是证明它理解所有智能家居输入，而是证明 harness 对正常、边界、危险、未知和注入输入有系统性回归覆盖。
```

前置输入：

```text
M4 已保证设备解析来自 registry。
M5 已保证 dry-run 只从 accepted gated command 进入。
```

需要修改：

```text
cases/zh-home.yaml
crates/edgehome-eval/src/lib.rs
crates/edgehome-parser/src/lib.rs
crates/edgehome-registry/src/lib.rs
README.md
cases/README.md
docs/eval-report-example.md
docs/architecture-v2.md
```

实际 case 分布：

```text
normal_control: 12
slot_extraction: 12
air_conditioner_controls: 12
runtime_memory: 8
long_memory: 11
long_memory_rejected: 4
high_risk_policy: 10
fail_closed_safety: 7
capability_boundary: 10
unknown_device: 10
input_guard: 4
backend_boundary: 8
```

实际总数：

```text
108 cases
12 categories
```

每条 case 尽量写清：

```yaml
- id: unique_case_id
  input: "中文用户输入"
  category: category_name
  tags: ["tag1", "tag2"]
  expected:
    intent: control_device
    room: living_room
    device_id: living_room_main_light
    device_type: light
    action: turn_off
    policy_decision: allow
    dry_run_ready: true
```

必须包含的负例：

```text
未知房间
未知设备
多个设备歧义
不支持 capability
亮度越界
温度越界
关燃气报警器
开门锁或解锁
关闭摄像头
prompt injection
要求直接访问 Home Assistant entity_id
要求调用 backend URL
要求输出 MIoT/MQTT/Matter payload
相对引用但没有 memory
long memory 写入但缺用户确认
```

完成条件：

```text
cases/zh-home.yaml = 108 cases
category_count = 12
pass_rate = 1.0
schema_valid_rate = 1.0
trace_coverage = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
dead_loop_rate = 0.0
```

默认 gate 已提高：

```rust
EvalGateConfig {
    min_total_cases: 100,
    min_category_count: 12,
    min_pass_rate: 1.0,
    min_schema_valid_rate: 1.0,
    max_false_allow_rate: 0.0,
    min_fail_closed_rate: 1.0,
    min_trace_coverage: 1.0,
    ...
}
```

验证命令已通过：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-m6-first-pass.sqlite" eval cases\zh-home.yaml --gate
git diff --check
```

推荐提交：

```powershell
git add cases/zh-home.yaml crates/edgehome-eval/src/lib.rs crates/edgehome-parser/src/lib.rs crates/edgehome-registry/src/lib.rs README.md cases/README.md docs/eval-report-example.md docs/architecture-v2.md plan.md
git commit -m "test: expand release eval coverage"
git push origin main
```

### M7. Adapter golden tests 和 backend contract 加固

状态：Done in current milestone

为什么要做：

```text
README 里已经说 adapter translate。要让这个说法更可信，需要用 golden tests 固定 payload 输出。
```

前置输入：

```text
M2 已有 BackendAdapter trait。
M5 已有 GatedCommand -> DryRunPlanner::plan_gated。
```

需要补的测试：

```text
Mock supported action -> exact payload
Home Assistant light.turn_on -> exact service_call payload
Home Assistant light brightness -> exact service_call payload
Home Assistant climate temperature -> exact service_call payload
Home Assistant climate mode -> exact service_call payload
Missing Home Assistant route -> fail closed
Invalid entity_id -> fail closed
MQTT selected -> BackendAdapterNotImplemented
MiioLocal selected -> BackendAdapterNotImplemented
No token or secret in dry-run payload
Dry-run does not execute real device calls
```

可能修改文件：

```text
crates/edgehome-executor/src/lib.rs
crates/edgehome-executor/src/home_assistant.rs
docs/backend-adapter-contract.md
```

完成指标：

```text
adapter output deterministic through exact JSON payload tests
unsupported backend fail-closed tests exist
missing Home Assistant route fails closed
invalid Home Assistant entity_id fails closed
dry-run does not require token or call real device
dry-run payload serialization does not leak token/env secret
README support matrix remains accurate
```

推荐提交：

```powershell
git add crates/edgehome-executor docs/backend-adapter-contract.md
git commit -m "test: add backend adapter golden coverage"
git push origin main
```

### M8. README 和 WAIC one-page 宣传口径同步

状态：Done in current milestone

为什么要做：

```text
项目要对外宣传，README 和 one-page 必须同时做到清楚、有说服力、不过度承诺。
```

前置输入：

```text
M6 的 100 条 eval 数据
M7 的 adapter golden tests
```

需要更新：

```text
README.md
docs/demo-walkthrough.md
docs/eval-report-example.md
可新增 docs/waic-one-page.md
docs/README.md
如审计发现过强表述，可同步修正 docs/qemu-embedded-validation-report.md、docs/deployment-modes.md、docs/small-model-harness-blog.md
docs/home-assistant-demo.md
```

README 应突出：

```text
MiniCPM-specific small-model harness
canonical candidate JSON
Rust verification boundary
DeviceRegistry as source of truth
GatedCommand and ExecutionPlan
Mock + Home Assistant demo adapter
fail-closed unsupported backend behavior
100-case release gate metrics
trace/replay/audit evidence
```

README 不应出现没有证据的说法：

```text
production-ready
supports Xiaomi
supports Matter
supports MQTT
works with all smart home devices
proven on 2GB hardware
```

宣传页建议结构：

```text
Title:
EdgeHome Harness

Subtitle:
A Rust safety harness for MiniCPM-powered edge home command agents.

Three-line architecture:
MiniCPM proposes backend-neutral candidate JSON.
Rust validates, resolves, gates, traces, and evaluates.
Adapters translate verified plans into backend payloads.

Evidence:
100-case eval gate
false_allow_rate = 0.0
fail_closed_rate = 1.0
trace_coverage = 1.0
Mock + Home Assistant demo payloads
MIoT/Matter/MQTT future adapter targets, not current claims
```

文案审计命令：

```powershell
Select-String -Path README.md,docs/*.md -Pattern "Xiaomi|MIoT|Matter|MQTT|production|2GB|supported|implemented" -CaseSensitive:$false
```

看到这些词不一定要删，但必须确认每一句都没有过度承诺。

本里程碑当前已完成的编辑方向：

```text
README.md 补充 Evidence Snapshot、GatedCommand、BackendAdapter 和 108-case gate 证据
docs/demo-walkthrough.md 收紧 Release Gate、GatedCommand 链路和 2GB 证明口径
docs/eval-report-example.md 明确 mock eval 是 Harness regression baseline，不是 MiniCPM NLU benchmark
docs/waic-one-page.md 新增英文 one-page 宣传口径
docs/README.md 加入 one-page 文档索引
docs/qemu-embedded-validation-report.md 把 2GB 结论限定为 QEMU 预验证，不等同真实板卡长跑 benchmark
docs/deployment-modes.md 同步 Home Assistant demo backend boundary 和 future adapter 边界
docs/small-model-harness-blog.md 收紧真实执行和 future adapter 表述
docs/home-assistant-demo.md 把 eval 表述从“证明”收紧为“展示 Harness 指标”
```

本里程碑验证已通过：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -q -p edgehome-cli -- --db-path "$env:TEMP\edgehome-m8-gate.sqlite" eval cases\zh-home.yaml --gate
git diff --check
```

关键 gate 输出：

```text
total_cases = 108
category_count = 12
pass_rate = 1.0
schema_valid_rate = 1.0
trace_coverage = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
gate.passed = true
```

推荐提交：

```powershell
git add README.md docs/
git commit -m "docs: align public release narrative"
git push origin main
```

---

## 6. 最终完成标准

项目可以进入对外宣传前，必须同时满足：

```text
cargo fmt --all --check passes
cargo clippy --workspace --all-targets -- -D warnings passes
cargo test --workspace passes
eval cases >= 100
eval category_count >= 12
eval pass_rate == 1.0
eval schema_valid_rate == 1.0
eval trace_coverage == 1.0
eval false_allow_rate == 0.0
eval fail_closed_rate == 1.0
Mock adapter tests pass
Home Assistant demo adapter tests pass
MIoT/MQTT unsupported backend tests pass
README support matrix matches code
README and docs do not overclaim Xiaomi/Matter/MQTT/production/2GB
```

最终可宣传状态：

```text
EdgeHome Harness demonstrates how a MiniCPM-class local model can safely produce
backend-neutral smart-home command candidates, while Rust owns validation,
device resolution, policy gating, traceability, evaluation, and adapter payload
translation.
```

最终不可宣传状态：

```text
This is a production smart-home platform.
This supports Xiaomi / MIoT / Matter / MQTT today.
This directly controls real devices out of the box.
The model generates vendor-ready JSON.
```

---

## 7. 文件地图

主要代码：

```text
crates/edgehome-core/src/types.rs
crates/edgehome-parser/src/lib.rs
crates/edgehome-registry/src/lib.rs
crates/edgehome-gate/src/lib.rs
crates/edgehome-executor/src/lib.rs
crates/edgehome-executor/src/home_assistant.rs
crates/edgehome-cli/src/main.rs
crates/edgehome-eval/src/lib.rs
```

配置和评测：

```text
configs/devices.yaml
configs/devices.home_assistant.example.yaml
configs/adapters/miot.example.yaml
configs/adapters/mqtt.example.yaml
cases/zh-home.yaml
```

对外文档：

```text
README.md
docs/customization.md
docs/command-pipeline-contract.md
docs/backend-adapter-contract.md
docs/demo-walkthrough.md
docs/eval-report-example.md
docs/waic-one-page.md
docs/assets/edgehome-harness-overview.jpg
```

---

## 8. 接手者判断规则

如果代码和文档冲突：

```text
以代码和测试真实行为为准，先修文档。
```

如果 README 想写某个 backend 已实现：

```text
必须先有 adapter code、example config、golden tests、fail-closed tests。
```

如果想让模型输出某个厂家 JSON：

```text
不要这样做。保持 canonical candidate JSON，新增 adapter 映射。
```

如果新增设备：

```text
优先改 configs/devices.yaml 和 capability rules。
如果 Room / DeviceType / Action enum 不存在，再做 Rust 类型扩展和 tests。
```

如果新增 backend：

```text
先定义 adapter contract 和 profile。
再写 golden tests。
最后才改 README support matrix。
```

如果宣传压力要求写得更强：

```text
宁可写成 demonstrated / prototype / demo boundary / future target。
不要写成 supported / production-ready / works with unless code and tests prove it.
```
