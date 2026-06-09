# EdgeHome Harness 计划

## 1. 项目目标

构建一个基于 Rust 的 Harness，用于 1B 级端侧语言模型，在 2GB RAM 约束下，将智能家居和 IoT 指令解析成安全、可校验、可审计的动作。

项目聚焦：

```text
1B 端侧小模型
智能家居 / IoT 指令
JSON / intent 解析
业务校验
本地执行安全
2GB RAM 稳定性
MiniCPM5 / Qwen 评测
```

第一个运行时集成将使用 Ollama structured outputs。

第一组模型对比将是：

```text
MiniCPM5-1B
Qwen3.5-0.8B
```

## 2. 核心原则

Ollama structured output 保证 JSON 语法。EdgeHome Harness 保证本地动作安全。

```text
Ollama JSON 约束：
  校验格式

EdgeHome Harness：
  校验业务含义
  校验设备能力
  强制执行策略
  防止不安全执行
  防止失控的运行时行为
  提供审计日志
```

项目应始终将模型输出视为不可信输入。

## 3. MVP 定义

当下面这条命令可以端到端工作时，MVP 即完成：

```bash
edgehome dry-run "晚上十点后把走廊灯调到30%"
```

期望输出：

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

MVP 必须包含：

```text
Rust CLI
Ollama adapter
模型输出清洗器
JSON 提取
schema validation
语义标准化
业务校验
策略决策
mock dry-run executor
SQLite 审计日志
YAML eval runner
MiniCPM5 vs Qwen 测试用例
```

MVP 不应包含：

```text
真实门锁控制
真实燃气设备控制
多步自主 Agent loop
云端依赖
Node.js daemon
Python 生产服务
```

## 4. 里程碑

### Milestone 0：仓库基础

任务：

```text
创建 Rust workspace
创建 crate 结构
添加格式化和 lint 配置
添加 README 和 plan
后续添加初始 CI
```

Workspace 结构：

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

验收标准：

```text
cargo check 通过
edgehome CLI binary 可以构建
```

### Milestone 1：核心类型与 Schema

任务：

```text
定义 Intent enum
定义 Room enum
定义 Device enum
定义 Action enum
定义 RiskLevel enum
定义 Command struct
定义 ModelCandidate struct
定义 PolicyDecision enum
定义 ExecutionPlan struct
```

初始 intents：

```text
turn_on
turn_off
set_brightness
set_temperature
query_status
create_rule
unknown
```

初始 rooms：

```text
living_room
bedroom
hallway
kitchen
bathroom
unknown
```

初始 devices：

```text
light
air_conditioner
curtain
camera
lock
unknown
```

验收标准：

```text
所有核心类型都可以 serialize 和 deserialize
可以生成或维护给 Ollama format 使用的 JSON schema
单元测试覆盖 enum parsing 和 serialization
```

### Milestone 2：Ollama Structured Output Adapter

任务：

```text
调用 Ollama /api/chat
支持模型名配置
支持 JSON schema format
支持 timeout
支持 temperature/top_p/top_k/repeat_penalty/num_predict
先支持 non-streaming
捕获原始模型输出
```

默认模型：

```text
openbmb/minicpm5:q4_K_M
qwen3.5:0.8b 或本地等价模型
```

验收标准：

```text
edgehome parse 可以调用 Ollama
原始输出会被保存
模型错误会以结构化错误返回
timeout 生效
```

### Milestone 3：输出清洗器和 Parser

任务：

```text
移除 <think>...</think>
移除 markdown 代码块
提取第一个 JSON 对象
处理 JSON 前后的多余文本
找不到 JSON 时 fail closed
```

需要处理的示例：

```text
<think>...</think>{"intent":"turn_off"}
```json
{"intent":"turn_off"}
```
Here is the JSON: {"intent":"turn_off"}
```

验收标准：

```text
cleaner 单元测试通过
非法输出不会 panic
非法输出不会被执行
```

### Milestone 4：语义标准化

任务：

```text
中文房间映射
中文设备映射
中文动作映射
亮度提取
温度提取
时间表达标准化
设备别名处理
```

示例：

```text
走廊 -> hallway
客厅 -> living_room
走廊灯 -> room=hallway, device=light
关掉 -> turn_off
调到 -> 当目标是 light 时为 set_brightness
30% -> 30
晚上十点后 -> 22:00
```

验收标准：

```text
normalization tests 覆盖常见中文智能家居指令
模型原始中文输出可以转换为 canonical command format
```

### Milestone 5：业务校验器

任务：

```text
设备注册表
设备能力表
数值范围校验
房间 / 设备存在性检查
动作兼容性检查
```

规则：

```text
light 支持 turn_on, turn_off, set_brightness
air_conditioner 支持 turn_on, turn_off, set_temperature
curtain 后续支持 open, close, set_position
lock 的 lock/unlock 需要确认
camera 的 disable 需要确认
gas devices 被阻断
```

验收标准：

```text
非法 device/action 组合会被拒绝
不存在的设备会被拒绝
越界数值会被拒绝
```

### Milestone 6：Policy Engine

任务：

```text
分配风险等级
做出 allow/confirm/deny 决策
支持策略配置
对 unknown actions 默认拒绝
```

风险模型：

```text
read -> allow
low -> allow
medium -> audit 或 confirm
high -> require confirmation
blocked -> deny
```

验收标准：

```text
lock/camera 动作需要确认
gas/medical/critical devices 被拒绝
unknown devices 和 unknown actions 被拒绝
```

### Milestone 7：Mock Executor 和 Dry Run

任务：

```text
创建 Executor trait
实现 MockExecutor
实现 dry-run 输出
默认阻止直接执行
```

验收标准：

```text
edgehome dry-run 返回可执行计划
edgehome execute 在没有明确确认时拒绝高风险指令
MVP 不控制真实设备
```

### Milestone 8：审计日志

任务：

```text
SQLite 审计数据库
存储原始输入
存储模型名
存储原始模型输出
存储清洗后的输出
存储标准化后的指令
存储校验结果
存储策略决策
存储 dry-run 结果
存储延迟
```

验收标准：

```text
每次 parse/dry-run 都写入一条审计记录
审计记录可以通过 CLI 查询
```

### Milestone 9：Eval Runner

任务：

```text
读取 YAML cases
运行 parse pipeline
对比 expected command fields
计算 metrics
对比 MiniCPM5 和 Qwen
导出 JSON/Markdown report
```

指标：

```text
intent 准确率
room 准确率
device 准确率
action 准确率
slot 准确率
schema 合法率
标准化成功率
策略正确率
平均延迟
p95 延迟
死循环或 timeout 数量
```

验收标准：

```text
edgehome eval cases/zh-home.yaml 可以运行
报告对比 MiniCPM5-1B 和 Qwen3.5-0.8B
```

### Milestone 10：2GB RAM Profile

任务：

```text
记录测试环境
使用 MiniCPM5-1B Q4 运行
使用 Qwen3.5-0.8B 运行
测量 RSS 和延迟
记录 context length 和 num_predict 设置
在受限内存下测试
```

目标设置：

```text
num_ctx=1024
num_predict=80-128
temperature=0-0.2
headless Linux
无桌面 UI
Rust daemon
model sidecar
```

验收标准：

```text
2GB profile 已记录
内存峰值已记录
延迟已记录
失败模式已记录
```

## 5. 初始测试用例

创建 `cases/zh-home.yaml`，包含示例：

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

## 6. 开发顺序

推荐顺序：

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

在 policy、audit 和 eval 工作之前，不要实现真实设备控制。

## 7. 模型策略

MiniCPM5-1B 是主要的纯文本候选模型。

Qwen3.5-0.8B 是对比候选模型，并且在需要多模态支持时可能有价值。

Harness 必须假设：

```text
模型输出不可信
模型输出可能包含 think blocks
模型输出可能包含 markdown
模型输出可能语义错误
模型输出可能很慢或 timeout
模型输出可能是合法 JSON 但不安全
```

## 8. 第一成功标准

第一个真正的成功不是聊天 demo。

第一个真正的成功是：

```text
edgehome eval cases/zh-home.yaml
```

展示：

```text
MiniCPM5-1B 和 Qwen3.5-0.8B 对比
valid JSON rate
intent/slot accuracy
policy correctness
latency
memory profile
```

第二个成功是：

```text
edgehome dry-run "晚上十点后把走廊灯调到30%"
```

返回一个不会控制真实设备的安全执行计划。

## 9. 后续扩展

MVP 之后：

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

在 MVP pipeline 稳定之前，不要添加这些内容。

