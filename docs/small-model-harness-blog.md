# 为什么我们要做一个小模型 Harness

> EdgeHome Harness 是一个面向 1B 端侧小模型的 Rust Agent Harness 项目。它的产品形态是智能家居本地控制中台，但核心目标不是做一个新的语音助手，而是探索低资源设备上如何把小模型稳定约束进真实执行链路。

## 一个不太一样的 Agent 方向

过去一段时间，Agent 工程的主流叙事大多围绕云端强模型展开：更大的上下文、更强的推理能力、更复杂的工具调用、更长的任务规划，以及以 code agent 为代表的高复杂度工作流。

这些方向当然重要，但它们并不能覆盖所有真实场景。

在智能家居、工业设备、企业内网流程、涉密系统、低延迟本地控制等场景里，用户真正需要的往往不是一个开放式聊天模型，而是一个足够稳定、足够可控、足够本地化的执行系统。

这类场景有几个共同点：

```text
隐私和合规要求更高
不一定允许把数据送到云端
响应延迟不能太高
设备动作必须可控
失败需要可回放
硬件资源通常有限
```

所以我们开始关注另一个问题：

```text
如果未来很多垂直场景会下沉到端侧，
那一个 1B 级别的小模型，怎样才能被安全地放进真实业务链路里？
```

EdgeHome Harness 就是围绕这个问题做的工程实验。

## 小模型的问题不是不会说话，而是不够可靠

1B 小模型已经可以完成很多短指令任务，例如意图分类、槽位抽取、简单 JSON 生成。

比如：

```text
用户：把卧室空调打开
模型可以输出：
{
  "intent": "turn_on",
  "room": "bedroom",
  "device": "air_conditioner"
}
```

但小模型一旦进入真实执行链路，问题会马上暴露出来：

```text
输出 JSON 格式漂移
字段含义不稳定
复杂槽位容易漏
多轮指代容易错
开放聊天容易空话
稍微放开输出就可能复读或死循环
遇到不确定设备时可能胡编
低内存设备上上下文不能无限增长
```

这说明一个事实：小模型可以参与执行链路，但不能直接支配执行链路。

所以 EdgeHome Harness 的核心原则是：

```text
LLM only generates candidate JSON.
Rust Harness owns truth, policy, memory, execution and observability.
```

换成人话就是：

```text
模型只负责给候选答案。
真正能不能执行，必须由 Harness 判断。
```

## 为什么不是只用 Ollama structured outputs

Ollama structured outputs 可以让模型更容易输出合法 JSON。这很有价值，但它只解决了语法层面的问题。

它不能解决这些问题：

```text
设备是否存在
房间是否存在
动作是否被设备支持
亮度、温度、时间是否合法
这次动作是否属于高风险动作
短时记忆是否正确解析了“刚才那个”
模型是否正在复读
输出是否过长
是否应该重试
失败是否能回放
模型参数变更后是否退化
低内存时是否应该降级
```

这也是 Harness 的价值所在。

Ollama structured outputs 解决的是：

```text
格式尽量正确
```

EdgeHome Harness 解决的是：

```text
业务稳定
设备可控
执行安全
系统不死
失败可回放
版本可评测
```

二者不是替代关系，而是上下游关系。Ollama 可以作为候选 JSON 的生成器，而 Harness 负责把候选 JSON 变成可验证的 ExecutionPlan。

## 为什么选择智能家居作为产品形态

EdgeHome Harness 的本质是小模型 Harness 项目，智能家居只是我们选择的垂直场景。

选择智能家居有几个原因。

第一，它天然适合端侧。

家庭设备控制涉及个人隐私、生活习惯、设备状态和局域网资源。很多操作并不需要云端强推理，反而更需要本地响应、稳定执行和明确边界。

第二，它对小模型友好。

智能家居指令通常是短指令：

```text
打开空调
关闭客厅灯
把卧室灯调到 30%
睡觉模式
再暗一点
把刚才那个关掉
```

这类任务不需要大模型写长文，也不需要复杂规划。它更需要意图识别、槽位抽取、状态承接和安全执行。

第三，它能暴露真实 Harness 问题。

如果模型输出错了，系统不能随便执行。如果设备不存在，系统不能编一个设备。如果用户说“再暗一点”，系统需要知道上一次操作对象。如果模型死循环，系统必须能打断。如果低内存，系统要能降级。

这些问题刚好能检验 Harness 的工程能力。

我们不是要替代米家 App、小米音箱或 Home Assistant。更准确的定位是：

```text
在本地边缘设备上做一个小模型控制中台，
用智能家居场景验证端侧 Agent Harness 的完整链路。
```

## 我们现在做了什么

EdgeHome Harness 当前已经形成了一条完整的工程主链路：

```text
用户中文指令
-> Input Guard
-> Rule Parser
-> Runtime Memory
-> Context Compiler
-> MiniCPM5-1B / Ollama Candidate JSON
-> Output Governor
-> JSON Parser
-> Schema Validator
-> Semantic Normalizer
-> Device Registry
-> Policy Gate
-> Execution Planner
-> Executor Router
-> Trace Recorder
-> Eval / Replay
```

这条链路的重点不是让模型“自由发挥”，而是把模型限制在一个很窄、很明确、很容易验证的任务里。

当前项目包含几个核心模块。

### Runtime Memory

Runtime Memory 负责在线交互体验。

它不保存完整聊天历史，也不依赖 Ollama CLI 的上下文，而是由 Rust 侧维护结构化状态。

典型状态包括：

```text
last_target
last_action
last_value
device alias
room alias
user preference
scene preference
```

它解决的是：

```text
再暗一点
再调高一点
把刚才那个关掉
切换成除湿模式
恢复刚才的设置
```

长期偏好存储在 SQLite 中，不会每轮全部塞进 prompt。每次请求前，Context Compiler 只注入和当前输入相关的极短摘要。

这适合 1B 小模型，也适合 2GB RAM 级边缘设备。

### Output Governor

Output Governor 专门处理小模型不稳定输出。

1B 小模型不是越放开越好。稍微放开，它可能会复读、解释、跑题、输出过长，甚至陷入死循环。

所以我们需要治理输出：

```text
限制最大输出长度
检测重复片段
控制重试次数
限制 num_predict
设置 timeout
清洗 JSON 外包裹文本
失败时进入 fallback
必要时降级到 rule-only
```

这是小模型 Harness 和大模型 Harness 的一个核心差异。

大模型 Harness 常常默认模型能力足够强，重点放在规划和工具调用。小模型 Harness 必须默认模型会犯错，并提前设计失败路径。

### Device Registry / Policy Gate

模型输出永远不是命令。

模型输出只是候选结构，真正的执行计划必须经过设备注册表和策略层。

Device Registry 负责回答：

```text
这个房间存在吗
这个设备存在吗
这个设备支持这个动作吗
这个参数是否在能力范围内
这个别名能否映射到真实设备
```

Policy Gate 负责回答：

```text
这个动作是否允许自动执行
是否只能 dry-run
是否需要确认
是否属于禁止动作
低内存或离线状态下是否应该拒绝
```

安全判断不交给小模型。小模型只负责候选 JSON，确定性边界必须由 Rust 代码维护。

### ExecutionPlan

经过校验后的动作不会直接变成设备调用，而是先变成 ExecutionPlan。

ExecutionPlan 的意义是把“模型想做什么”和“系统实际允许做什么”分开。

它可以表达：

```text
allow
dry_run
reject
fallback
need_confirmation
```

这让系统可以在真实执行前进行审计、测试和回放。

### Trace / Replay / Eval

EdgeHome Harness 不把 Evidence 放在普通动作的在线门禁里。

智能家居控制需要低延迟。用户说“打开空调”，系统不应该像企业报销审批一样每次都走复杂证据链。

所以我们采用现在的定位：

```text
Memory for runtime.
Evidence for debugging.
Eval for progress.
Policy for deterministic hard constraints.
LLM for candidate JSON only.
```

Trace 记录每次请求的关键过程：

```text
原始输入
模型参数
运行 profile
记忆摘要
模型原始输出
清洗后的 JSON
schema 校验结果
归一化命令
设备解析结果
策略判断
执行计划
执行结果
失败原因
重试次数
延迟
内存压力
```

Replay 用于复盘失败。

Eval 用于比较不同模型、不同参数、不同 prompt、不同策略是否退化。

这让项目不是靠感觉调 prompt，而是能形成工程闭环。

## 为什么用 Rust

这个项目选择 Rust，不是为了炫技，而是因为端侧 Harness 需要这些能力：

```text
低运行时开销
可控内存
强类型边界
静态二进制部署
系统服务化
SQLite 本地存储
本地 SDK / runtime 集成
适合长期常驻
```

TypeScript 和 Python 很适合快速做 Agent 原型，但如果目标是 2GB RAM 级边缘设备、本地控制网关、系统 SDK 和长期服务，Rust 更能表达这个项目的工程取向。

EdgeHome Harness 想展示的是：

```text
端侧 Agent 不是把云端 Agent 搬到小板子上，
而是重新设计模型职责、内存边界、执行边界和失败边界。
```

## 2GB RAM 是约束，不是口号

我们把 2GB RAM 级边缘设备作为设计约束，但不把它宣传成“2GB 流畅运行一切”。

真实判断是：

```text
2GB 可以作为 low_memory profile 的极限验证目标
4GB 更适合实际部署和演示
8GB 更适合开发、评测和多服务联调
```

在 2GB 约束下，系统必须控制：

```text
上下文长度
输出长度
记忆注入大小
重试次数
日志内存占用
并发数量
模型参数
fallback 策略
```

所以项目里有 low_memory profile 和内存压力降级策略。

低内存时，系统可以逐步降级：

```text
full_json -> compact_json -> rule_only
memory_enabled -> memory_limited -> memory_disabled
num_ctx 下调
num_predict 下调
长期记忆注入关闭
只保留确定性规则路径
```

这比单纯说“模型可以跑在树莓派上”更有意义。

## 当前边界

为了避免过度宣传，我们也明确当前项目边界。

当前项目不是：

```text
完整商业智能家居网关
米家 App 替代品
小米音箱替代品
Home Assistant 替代品
全量 MIoT / miIO / Matter 适配器
已经完成真实 2GB ARM 板长期压测的产品
```

当前项目是：

```text
一个可复现的 Rust 小模型 Harness 工程原型
一个智能家居本地控制场景下的端侧 Agent 实验
一个验证 1B 小模型如何进入可控执行链路的系统
一个面向低资源设备的 Memory / Policy / Trace / Eval 工程样例
```

这个边界非常重要。

我们现在做的是 Harness 能力，而不是一开始就承诺完整设备生态。

## 接下来的目标

后续我们会继续推进几个方向。

第一，真实小模型接入。

以 MiniCPM5-1B 为默认模型，通过 Ollama structured outputs 生成候选 JSON，并持续比较不同参数、不同 profile 下的稳定性。

第二，真实边缘设备验证。

推荐部署目标是 Raspberry Pi 5 4GB，项目保留 2GB RAM 级 low_memory profile。我们会重点验证短上下文、短输出、低内存降级、SQLite 记忆和 Rust 常驻服务。

第三，智能家居执行器扩展。

当前默认 MockExecutor，Home Assistant 作为 demo backend。后续可以继续扩展 MQTT、Matter、MIoT、miIO 或其他本地设备协议，但不会把某一个执行器和 Harness 主体绑定死。

第四，评测体系完善。

我们会继续补充中文智能家居 case，覆盖：

```text
单轮指令
多轮指代
别名解析
参数边界
危险动作
低内存降级
模型死循环
JSON 漂移
策略拒绝
fallback
```

第五，形成可展示的面试 Demo。

最终希望面试官能看到的不只是“模型输出了 JSON”，而是完整链路：

```text
小模型输出不可靠
Harness 如何接管
失败如何记录
策略如何拒绝
记忆如何参与
低内存如何降级
评测如何证明没有退化
```

## 最后

EdgeHome Harness 不是为了证明 1B 小模型可以替代大模型。

它想证明的是另一件事：

```text
在垂直场景里，小模型不需要无所不能。
只要 Harness 设计正确，小模型可以负责候选生成；
业务真相、记忆、策略、执行和观测交给确定性系统。
```

这可能是端侧 Agent 真正落地时更现实的形态。

不是把一个大模型塞进小设备里，而是让一个小模型在严格边界内发挥价值。

这就是我们现在正在做的小模型 Harness。
