# Demo Walkthrough

本文是 EdgeHome Harness 的面试演示脚本说明。

演示目标不是展示“智能家居能开关灯”，而是展示：

```text
1B 小模型只生成候选 JSON。
Rust Harness 负责输出治理、记忆、设备解析、policy、dry-run、trace、replay、eval gate。
智能家居只是这个 Harness 的垂直落地场景。
```

## 运行命令

```powershell
$env:CARGO_TARGET_DIR="$env:TEMP\edgehome-target"
powershell -ExecutionPolicy Bypass -File scripts\demo.ps1 -DatabasePath edgehome-demo.sqlite
```

脚本默认使用 `MockExecutor` 和 mock model，不依赖真实设备。

## 演示顺序

### 1. Release Gate

命令：

```powershell
edgehome eval cases/zh-home.yaml --gate
```

展示点：

```text
当前 mock eval 覆盖 108 条 case、12 个 category。
eval 不只看模型输出是否正确，还看 schema、trace、retry、dead_loop、memory_resolution、false_allow、fail_closed 等 Harness 指标。
gate.passed = true 才说明当前版本没有破坏已覆盖的 Harness 主链路。
```

### 2. 普通指令

输入：

```text
把客厅灯关掉
```

展示点：

```text
中文指令 -> 候选 JSON -> 归一化命令 -> DeviceResolver -> GateEngine -> GatedCommand -> ExecutionPlan -> BackendAdapter payload。
模型输出不是命令，只有 gate 接受后的 GatedCommand 才能进入 dry-run planner。
```

### 3. 槽位抽取

输入：

```text
晚上十点后把走廊灯调到30%
```

展示点：

```text
room = hallway
device_id = hallway_light
action = set_brightness
brightness = 30
time_after = 22:00
```

这对应小模型在窄场景下最有价值的工作：短 JSON 槽位抽取。

### 4. Trace Replay / Trace Export

命令：

```powershell
edgehome replay <trace_id>
edgehome trace export <trace_id>
```

展示点：

```text
TraceFrame 能看到 input、model params、schema_result、normalized_command、device_resolution、capability_result、execution_plan、latency。
Evidence 不 gate 普通动作，但 gate 版本质量和失败复盘。
```

### 5. 短时记忆

输入链：

```text
晚上十点后把走廊灯调到30%
把刚才那个灯再调暗一点
```

展示点：

```text
短时记忆保存 last_target。
第二句依靠 Rust 结构化记忆补全设备，而不是依赖 Ollama 聊天历史。
```

当前设备表没有卧室灯，所以脚本使用走廊灯演示同一类短时指代能力。

### 6. 长期别名记忆

输入链：

```text
以后把玄关灯叫小夜灯
打开小夜灯
```

展示点：

```text
长期记忆必须由用户明确表达写入。
别名保存在 SQLite。
每轮请求只注入极短摘要，不把长期记忆整库塞进 prompt。
```

### 7. 危险动作拒绝

输入：

```text
关闭燃气报警器
```

展示点：

```text
1B 小模型不判断风险。
风险来自 Device Registry / policy config。
blocked risk 会被 PolicyGate 拒绝，dry_run_plan = null。
```

### 8. 2GB 降级策略

命令：

```powershell
edgehome config pressure --free-memory-mb 1024
edgehome config pressure --free-memory-mb 400
edgehome config pressure --free-memory-mb 128
```

展示点：

```text
normal：保留 low_memory profile。
elevated：压缩 num_ctx / num_predict，fallback 到 compact_json。
critical：num_ctx<=512，num_predict<=64，memory_enabled=false，fallback 到 rule_only。
```

这展示 low-memory profile、上下文预算、输出预算和压力降级是代码路径，不只是 README 口号。
真实 2GB ARM 板卡上的长时间稳定性仍需要单独 benchmark。

### 9. Output Governor 失败恢复

命令：

```powershell
cargo test -q -p edgehome-ollama output_governor_report_classifies_dead_loop_and_fallback
```

展示点：

```text
1B 小模型可能复读、死循环、输出坏 JSON。
OutputGovernor 会识别 dead_loop，并给出 fallback 建议。
失败不会绕过 schema / policy / executor。
```

## 面试讲法

可以按这个顺序讲：

```text
第一，我没有做一个“模型输出 JSON 就执行”的玩具 demo。
第二，我把模型限制在候选 JSON 生成，业务真相由 Rust Harness 管。
第三，我实现了 Runtime Memory，所以能处理“刚才那个”“小夜灯”这类真实家居对话。
第四，我用 OutputGovernor 处理 1B 小模型容易死循环、复读、坏 JSON 的问题。
第五，我用 Device Registry、capability、policy 和 ExecutionPlan 保证设备可控。
第六，我用 Trace / Replay / Eval / Release Gate 让失败可复盘、版本可回归。
第七，我把 2GB RAM 约束做成 profile、上下文预算、输出预算和降级策略；真实硬件长跑 benchmark 另算。
```

## 非目标

演示时不要说：

```text
本项目替代米家 App。
本项目替代小米音箱。
本项目替代 Home Assistant。
所有米家设备都能纯离线控制。
1B 小模型可以开放聊天。
```

正确说法：

```text
这是一个面向端侧小模型的 Agent Harness 项目。
智能家居是垂直场景。
Home Assistant / MQTT / MIoT bridge / Matter bridge 都是 adapter boundary。
真实设备执行默认关闭，优先展示 dry-run、trace、replay、gate 和 explicit execute 边界。
```
