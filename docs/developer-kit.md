# 从模板开发自己的端侧 Agent

`edgehome-kit` 为通用 SDK 提供项目初始化、配置诊断、命令行运行、stdio RPC 和固定任务评测。生成的项目拥有自己的工具代码，运行时和推理客户端通过本地 Cargo 路径依赖复用。

## 创建独立应用

在 EdgeHome 仓库根目录运行：

```sh
cargo run --locked -p edgehome-kit -- init ../my-edge-agent --name my-edge-agent
cd ../my-edge-agent
cargo run -- doctor --mock
cargo run -- tools
```

`init` 只接受尚不存在的目标目录，不覆盖现有文件。默认引用当前 SDK 源码位置；需要指定另一个检出目录时使用 `--sdk-path /path/to/EdgeHome-Harness`。生成结果为：

```text
my-edge-agent/
  Cargo.toml       独立 workspace 和 SDK 路径依赖
  agent.yaml       模型配置与任务预算
  src/main.rs     共用入口
  src/tools.rs    应用自己的工具与确定性 demo provider
  cases.json      8 条工具/参数/结果/澄清回归用例
  client.py       Python stdio 集成示例
  README.md
  LICENSE
```

SDK 源码目录需要继续存在。当前模板未依赖已发布的 crates.io 包，也没有后台安装模型、下载依赖包或启动推理服务。第一次 Cargo 构建可能下载普通 Rust 依赖。

## 先完成一个短任务

```sh
cargo run -- run --mock --prompt "Count the sample document" --tool document.stats
cargo run -- run --mock --prompt "Count the sample document" --tool document.stats --allow document.stats
cargo run -- run --mock --prompt "Read the room sensor" --tool sensor.read --allow sensor.read --events
```

- 第一条只提出计划，不执行。
- `--tool` 可以重复，限定模型本次可见、可调用的工具。即使模型输出了其他已注册工具，宿主仍拒绝它。
- `--allow` 可以重复，表示宿主对本次进程中指定工具的预授权。它不绕过参数和业务校验；参数范围应由工具校验器限制。
- 默认执行成功后直接返回工具回执，省掉模型转述。需要继续模型循环时显式增加 `--loop`，仍受实例总调用预算限制。
- `--mock` 始终是显式选择的确定性 demo，不是模型失败后的回退。

每次 `run` 都是新的进程实例，预览计划不能带到另一次命令中恢复。`--allow` 是对新调用的工具预授权；需要确认已经展示的那个具体计划时，用下面的 RPC 或直接嵌入 SDK，保持同一实例。

模板工具只处理内置文档与固定温度样例；它们不读取用户文件或真实传感器。把 `src/tools.rs` 的参数类型、描述、校验器和处理函数替换为应用逻辑即可。无需修改运行时内部枚举。

参数结构应拒绝未知字段；范围、权限和状态检查写入无副作用的校验器。处理函数收到 `ToolContext`，自行落实 I/O 超时、输出上限和合作式取消。真正的文件或设备工具应限制操作对象，不能仅靠模型遵守描述。

## 接入真实模型

编辑 `agent.yaml` 中的 `inference`。以下示例使用 llama.cpp 服务：

```yaml
inference:
  engine: llama_cpp
  base_url: http://127.0.0.1:8080/v1
  model: your-served-model
  connect_timeout_ms: 2000
  request_timeout_ms: 15000
  max_response_bytes: 65536
  max_output_tokens: 256
runtime:
  max_model_calls: 8
  max_tool_calls: 4
  max_context_bytes: 16384
  require_tool_observation: true
```

其余[引擎预设](../configs/inference/ollama.yaml)的字段放在 `inference` 下。原 `document_agent --config` 读取的是扁平推理配置，开发工具读取的是含 `inference/runtime` 的应用配置，两者不要直接混用。

```sh
cargo run -- doctor
cargo run -- doctor --probe
cargo run -- run --prompt "Count the sample document" --tool document.stats --allow document.stats
```

`doctor` 验证本地配置、模型客户端构造和基本运行时预算，不发起网络请求。`--probe` 发起两次有时限、无工具的请求，要求读取仅存在于 system 消息中的不同测试值；答案不写进 schema，避免只检查 JSON 格式却漏掉聊天模板问题。它不会验证任务准确率、所有工具的 Schema 或硬件内存。`--config FILE` 可以覆盖默认的 `agent.yaml`。

默认请求时限为 15 秒；模型冷启动超过时限时，配置中显式增大预算或预加载模型。错误分类保留超时、截断、响应过大、无效候选等原因，不自动重试执行。

## 运行事件与会话记录

`--events` 将元数据事件输出到 stderr，stdout 留给结果或 RPC。事件包括任务开始、模型请求开始/结束、提案、执行开始、完成、取消与拒绝；模型结束事件只表示调用返回，不代表候选已通过校验。

```sh
cargo run -- run --mock --prompt "Count the sample document" --allow document.stats --events --journal session-001.jsonl
```

`--journal` 必须指向现有目录下的新文件。记录只保留事件类型、计划 ID 和工具名称，不写原始提示、参数或结果；默认内存记录最多 128 条，文件上限 512 KiB。文件写入会同步到存储，写入失败或达到上限时返回错误，派发前失败不会调用工具。

这是元数据会话日志，不能恢复模型上下文或重新执行历史计划。跨进程恢复、状态对账和完整对话存储仍由后续独立契约处理。

直接嵌入 Rust 时，可以通过 `ObservedJournal` 订阅事件，通过 `usage()` 查看模型/工具调用计数，通过 `cancellation_token()` 从其他线程发出停止信号。取消不可重置；需要后续工作时创建新实例。令牌传入自定义 provider 和工具，用于合作式停止。

**取消不会强制杀死阻塞函数，也不会撤销已经发生的副作用。** 内置同步 HTTP 客户端在回调边界检查令牌，等待期间仍依赖请求总时限。工具派发后观察到取消会报告 `OutcomeUnknown`，不能据此盲目重试。

## Python、Go 或其他宿主接入

启动子进程：

```sh
cargo run -- rpc --mock --enable-execution
```

协议为本地、顺序处理的 NDJSON v1，不是 JSON-RPC 2.0。每行一个 JSON 对象，请求最大 8,192 字节（含换行）；过大请求结束连接。没有网络监听端口，父进程是可信的授权边界。省略 `--enable-execution` 时，所有执行请求都被拒绝。

启动消息：

```json
{"type":"ready","protocol":1,"max_request_bytes":8192}
```

先取得计划：

```json
{"id":1,"command":{"method":"prompt","input":"Count the sample document","tools":["document.stats"]}}
```

宿主检查返回的 `result.plan`，完成用户确认或应用预授权后，再发送实际计划 ID：

```json
{"id":2,"command":{"method":"execute","plan_id":"RETURNED_PLAN_ID","approved":true}}
```

| 方法 | 请求参数 | 返回 |
| --- | --- | --- |
| `prompt` | `input`、可选 `tools` | 提案、回答或澄清；使旧计划失效 |
| `reply` | `clarification_id`、`input` | 保留同一任务上下文，用自然语言补充信息 |
| `resolve` | `clarification_id`、`values` | 填充请求的字段，重新校验并生成计划或新问题 |
| `execute` | `plan_id`、`approved`（默认 false） | 一次执行回执 |
| `continue` | 无 | 成功工具结果之后的下一步 |
| `status` | 无 | 当前预算使用、取消状态、待执行 ID 与待回答的结构化澄清 |
| `tools` | 无 | 工具定义与参数 Schema |
| `cancel` | 无 | 取消实例并撤销待执行计划 |

响应使用 `{"type":"result","id":1,"result":...}` 或 `{"type":"error","id":1,"code":"...","message":"..."}`。无法解析的请求使用 `id:null`，不回显原始输入。重复执行、过期计划、未授权执行和上下文超限均有明确错误码。

RPC 请求顺序处理，`cancel` 在被读取时生效，不能打断正在阻塞的前一条请求。取消后应重新启动进程。EOF 退出时不会自动执行任何待确认计划。使用 stdout 的应用工具应改为 stderr 日志，避免破坏协议。

新版本的 ready 消息还包含 `capabilities` 数组。澄清结果带有 `clarification` 对象；
ID 只在当前任务内有效。用户补充不会重置调用预算，也不会自动批准计划。
执行前状态变化时可能返回 `clarification_required`，此时旧计划已失效。
参见[真实文件应用与交互示例](file-agent.md)。

生成的 `client.py` 与仓库中的 [Python 示例](../examples/python/agent_client.py)只批准指定的只读样例工具，展示完整的提案与确认交换。Go 可以用 `os/exec` 和 `encoding/json` 实现同一协议；当前没有单独的 Go SDK。

## 固定任务评测

```sh
cargo run -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
cargo run -- eval --cases cases.json --allow document.stats --allow sensor.read
```

模板提供 6 条工具任务和 2 条缺失参数澄清任务。成功标准是正确工具、匹配参数、成功且记录完成的执行回执以及匹配的结构化结果；2 与 2.0 按同值比较，不转换字符串。schema v2 澄清案例还要求包含指定问题主题，但这个自动检查不能完整证明问题语义。

评测使用应用自己的工具实现。所有 `--allow` 授权的提案都会实际执行，包括与标准答案不符的提案；请使用无副作用的测试工具或隔离环境。答案只参与评分，不能帮助模型选工具或替模型拦错。

`--repeat 3 --order-seed 42` 对每个用例建立独立 Agent 并重复三轮，固定 seed 只用于执行顺序。`--include-responses` 显式记录原始模型内容、决策与执行回执，默认不输出这些内容。报告分别列出首轮成功、纠错恢复、错误提案、各轮及各类别成绩。[真实模型可靠性指南](model-reliability.md)提供 36 题验收集、配对实验和独立复算。

每个案例使用独立 Agent，报告模式、模型、逐例结果、实际耗时和调用计数，失败时进程返回非零。实际耗时包含该案例的模型请求及工具处理，不是纯推理吞吐指标。小样本不支持广泛模型排名，也不测进程 RSS 或真机长稳。

`--mock` 8/8 只验证工具与宿主流程。真实模型输出必须单独保存，不能把无效候选、意外澄清或正常退出当作任务成功。工具评测默认在回执后结束，不衡量模型最终回答和开放式多步规划。

## 验收与边界

```sh
python scripts/check-sdk-onboarding.py
```

该检查在仓库之外生成临时应用，编译两个业务工具，验证默认预览、显式执行、运行事件、Python RPC、8 条样例任务以及目录覆盖拒绝。临时项目使用系统配置的临时目录，构建复用 `CARGO_TARGET_DIR`；本工作环境均指向 G 盘。

当前没有动态扩展包加载、后台服务、TUI、跨进程恢复或进程沙箱。这里提供的是一个可运行、可复用的开发入口，边界由类型、宿主策略、预算和明确的结果状态共同约束。
