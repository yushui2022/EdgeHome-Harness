# 2GB RAM Profile

本文说明 EdgeHome Harness 如何按 2GB RAM 级别设备设计。

这不是“在 2GB 设备上无脑跑一个 1B 模型”的说明，而是说明 Harness 如何把小模型、记忆、输出预算、执行链路和降级策略全部限制在可控范围内。

当前仓库已经验证的是：

```text
Rust Harness 主链路可运行
low_memory profile 可加载
MiniCPM5-1B 参数有静态上限
短时记忆会被轮数和字符预算裁剪
OutputGovernor 会限制输出长度和死循环
内存压力策略会压缩 num_ctx / num_predict，临界时进入 rule-only
```

当前仓库没有伪造的结论是：

```text
尚未声明已经在真实 2GB ARM 板子上完成 benchmark
尚未声明 Home Assistant、Ollama、MiniCPM5 全部同机跑在 2GB 上
尚未声明所有米家设备都能纯离线控制
```

## 目标硬件边界

V1 目标设备是几百 MB 到 2GB RAM 的边缘网关或中控类设备。

2GB RAM 的现实约束是：

```text
系统本身会吃掉一部分内存
Ollama / runtime 会有常驻开销
1B INT4 权重仍然需要数百 MB 到 1GB 级内存
KV cache 会随 num_ctx 增长
JSON 输出越长，死循环风险和解析成本越高
长期记忆不能直接塞进 prompt
```

因此 EdgeHome Harness 的策略不是追求大上下文，而是追求：

```text
短 prompt
短输出
短记忆
强 schema
确定性校验
可回放 trace
失败时快速降级
```

## low_memory Profile

主配置文件：

```text
configs/low_memory.yaml
```

当前关键值：

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
dangerous_action_policy: deny
audit_enabled: true
trace_enabled: true
```

这些值的含义：

| 参数 | low_memory 值 | 目的 |
| --- | ---: | --- |
| `num_ctx` | 1024 | 限制 KV cache，避免长上下文撑爆内存 |
| `num_predict` | 128 | 智能家居 JSON 输出应该很短，防止死循环长输出 |
| `temperature` | 0.1 | 结构化指令任务优先稳定，不追求创造性 |
| `repeat_penalty` | 1.25 | 降低 1B 小模型复读概率 |
| `retry_count` | 1 | 小模型失败后只给一次修正机会，再降级 |
| `max_short_memory_turns` | 3 | 只保留最近少量命令上下文 |
| `max_context_chars` | 500 | 记忆注入按字符预算硬截断 |
| `executor_backend` | mock | 2GB 默认不真实执行，先 dry-run |

## 内存压力降级

M14 引入了运行时内存压力决策模型，位置：

```text
crates/edgehome-ollama/src/lib.rs
ResourcePressurePolicy
```

默认分三档：

| 空闲内存 | 档位 | 行为 |
| ---: | --- | --- |
| `> 512MB` | normal | 保持 low_memory profile |
| `257MB - 512MB` | elevated | `num_ctx <= 768`，`num_predict <= 96`，输出模式降到 compact JSON |
| `<= 256MB` | critical | `num_ctx <= 512`，`num_predict <= 64`，禁用记忆注入，进入 rule-only fallback |

对应代码决策会产生：

```text
MemoryPressureLevel
ResourcePressureDecision
adapted MiniCpm5Profile
memory_enabled
fallback_mode
reason
```

这满足项目里的核心要求：

```text
内存压力下会减少记忆注入
内存压力下会减少 num_ctx / num_predict 或进入 rule-only
```

注意：当前 V1 是策略模块和单元测试，不是常驻系统内存守护进程。
后续如果接 daemon，可以把系统可用内存采样接入 `ResourcePressurePolicy::adapt_profile`。

## 记忆系统的 2GB 约束

记忆系统不把长期记忆直接塞进 prompt。

当前约束：

```text
短时记忆：最多 3 轮
长期记忆：SQLite 持久化
prompt 注入：只注入摘要和 ref id
长期注入：ContextCompiler 内部最多取 3 条
总字符预算：max_context_chars <= 500
低资源 fallback：memory_enabled=false 时上下文为空
```

对应代码：

```text
crates/edgehome-memory/src/lib.rs
ContextAssembler
ContextCompiler
ContextAssemblerConfig
PromptContext
```

测试覆盖：

```text
context_assembler_respects_budget_and_uses_refs
context_assembler_can_disable_memory_for_low_resource_fallback
```

## 模型输出治理

1B 小模型最大的问题不是“不会说话”，而是：

```text
输出多余解释
输出畸形 JSON
输出重复片段
陷入 think / 循环
把危险命令说得像正常命令
```

低内存设备上不能让模型无限输出。

当前治理点：

```text
OutputGovernorConfig::max_output_bytes
OutputGovernorConfig::max_output_chars
detect_dead_loop
RetryPolicy
FallbackMode
ModelHealth circuit breaker
```

这和 Ollama structured outputs 的关系是：

```text
Ollama structured outputs 约束 JSON 语法
OutputGovernor 约束输出长度、复读、死循环、失败降级
Parser / schema 约束字段合法性
Gate / policy 约束业务安全
```

## SQLite 与 Trace

SQLite 在 2GB 设备上的角色：

```text
存 trace records
存 replay metadata
存 eval reports
存 audit events
存长期偏好和安全记忆
```

SQLite 不应该被当成高频时序数据库。

V2 只记录命令级 TraceFrame：

```text
raw_user_input
raw_model_output
parsed_json
normalized_command
device_registry_snapshot
capability_snapshot
device_state_snapshot
policy_rule_snapshot
dry_run_plan
executor_response
failure_reason
retry_count
latency_ms
memory_pressure
```

这让系统可以 replay，但不会把普通实时动作变成同步证据审批，也不会把设备每秒状态轮询全写进去。

## 验证命令

基础验证：

```powershell
$env:CARGO_TARGET_DIR="$env:TEMP\edgehome-target"
cargo fmt --all --check
cargo check
cargo test
```

low_memory profile：

```powershell
cargo run -p edgehome-cli -- config show
```

中文智能家居评测：

```powershell
cargo run -p edgehome-cli -- --db-path edgehome-m14-eval.sqlite eval cases/zh-home.yaml
```

期望：

```text
passed = 11
failed = 0
intent_accuracy = 1.0
slot_accuracy = 1.0
policy_accuracy = 1.0
dry_run_accuracy = 1.0
trace_coverage = 1.0
```

## 真实 2GB 板卡记录模板

后续真实板卡 benchmark 应按这个格式记录，不要只写“能跑”。

```text
设备型号：
CPU：
RAM：
OS：
架构：aarch64 / armv7 / x86_64
Rust binary size：
Ollama / runtime：
模型名：
量化版本：
权重大小：
num_ctx：
num_predict：
memory_enabled：
max_short_memory_turns：
max_context_chars：
空闲内存：
推理前 RSS：
推理中峰值 RSS：
推理后 RSS：
单条指令延迟：
eval 通过率：
是否触发 elevated pressure：
是否触发 critical pressure：
失败样例：
```

## 当前结论

EdgeHome Harness 的 2GB 设计重点不是把所有能力都塞进去，而是让每一层都有上限：

```text
模型有输出上限
上下文有字符上限
记忆有轮数上限
危险动作有 policy 上限
执行必须经过 ExecutionPlan
失败必须进入 fallback / audit
```

这才是端侧小模型 Harness 项目应该展示的工程含金量。
