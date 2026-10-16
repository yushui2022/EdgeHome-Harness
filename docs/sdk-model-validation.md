# 通用 SDK 模型验证记录

日期：2026-10-15。对象是通用 SDK，不是原有家居 CLI 的 108 条评测。

后续验证增加了动作优先协议、system 消息探针、36 题固定验收集、三次重复对照和独立复算。当前方法与完整证据入口见[真实模型可靠性](model-reliability.md)。以下记录保留为历史，不代表当前版本的验收成绩；历史 Qwen3 0.6B 导入模板也有配置缺陷，不能据此评价该模型的能力上限。

## 历史开发工具版：8 题单轮测试

增加 `edgehome-kit` 后，使用 [sdk-starter.json](../cases/sdk-starter.json) 对同一台机器上的三个模型运行了一轮固定测试。每个案例创建独立实例，只请求一次模型决定；正确工具和参数经宿主确认执行后，直接检查工具回执。工具可见范围由案例指定，不评估自动领域路由或后续模型转述。

| 模型 | 工具任务通过 / 6 | 澄清分支通过 / 2 | 合计 / 8 | 逐例机器记录 |
| --- | ---: | ---: | ---: | --- |
| Qwen3 0.6B Q4 | 4 | 0 | 4 | [报告](validation/sdk-starter-2026-10-15-qwen06.json) |
| MiniCPM5 | 1 | 2 | 3 | [报告](validation/sdk-starter-2026-10-15-minicpm.json) |
| Qwen3.5 0.8B | 0 | 1 | 1 | [报告](validation/sdk-starter-2026-10-15-qwen08.json) |

三次真实模型评测均因未全通过而返回退出码 1，没有把失败隐藏成正常完成。显式 mock 路径为 8/8，用于检查工具与宿主集成。仓库外生成应用的独立编译、两个工具调用和 Python RPC 交换也已通过。

Qwen 0.6B 完成三个文档案例和室内温度案例，但对室外温度意外澄清；两个缺失参数案例又没有正确澄清。MiniCPM 多数完整参数请求被误判为需要澄清。Qwen 0.8B 出现无效候选与意外澄清。这些结果说明当前 SDK 的任务边界可测，但不能宣称小模型已经稳定完成通用任务。

生成配置与逐例耗时包含在各报告中；不同模型生成预算仍采用下表中的 1,024/256 tokens，**不能把这组小样本作为公平模型排名**。本轮没有重试、预热控制、重复采样或 RSS 测量。澄清案例只校验分支，不评价自然语言问题的质量。[来源指纹](validation/sdk-starter-2026-10-15-manifest.json)记录案例与关键源码 SHA-256；运行时源码尚未提交，`base_commit` 仅用于定位起点。

以下保留较早的单文档探测，用于说明设计动机；它与上述固定任务集使用不同示例和提示，不合并计算指标。

## 环境与任务

- Windows 本地 Ollama `0.18.3`，使用机器上已存在的模型；本轮没有下载模型。
- 原生 `/api/chat`，非流式 JSON Schema 响应，temperature 0，context 2,048 tokens，整次请求时限 60 秒。
- HTTP 响应上限 65,536 字节，模型候选与工具结果各 4,096 字节。
- 示例要求先获得工具观察，最多执行一次只读工具；工具只访问内置字符串。
- 最终版本的提示：`Use document.stats for document sample, then report the character and line counts from its result.`
- 正确的工具参数为 `{"document":"sample"}`，确定性工具结果为 `{"characters":84,"lines":2}`。

模型身份来自本地 `/api/tags`，tag 后续可能变化：

| 模型 | 本次 digest | 生成设置 |
| --- | --- | --- |
| `openbmb/minicpm5:latest` | `08239e8f70e09fc079ff3b5c4f011fd846d452ae9ba4e4b4c40e526bd4950ded` | 最多 256 tokens，省略 `think` |
| `qwen3.5:0.8b` | `f3817196d142eaf72ce79dfebe53dcb20bd21da87ce13e138a8f8e10a866b3a4` | 最多 256 tokens，`think: false` |
| `mayulu/qwen3-0.6B-Q4_K_M:latest` | `f9beffc4cc0d395027022e797a0e496f10bcd017bdf7f0a666dafa4e4df9ee2e` | 最多 1,024 tokens，`think: false` |

## 最终提示下的观察

| 路径 | 工具执行 | 模型最终回答 | 判断 |
| --- | --- | --- | --- |
| 确定性示例 provider | 返回 84 字符、2 行 | 根据工具结果生成回答 | 验证 SDK 流程，不衡量模型能力 |
| MiniCPM5 / Ollama | 未调用 | 要求补充已经提供的文档名称 | 未完成任务；有效澄清信封不等于有效任务处理 |
| Qwen3.5 0.8B / Ollama | 未调用 | 候选被 `Decision` 校验拒绝 | 未完成任务 |
| Qwen3 0.6B Q4 / Ollama，完整循环 | 正确选择工具并返回 84 字符、2 行 | 返回无意义的澄清，未报告统计值 | 工具阶段成功，完整任务未完成 |
| Qwen3 0.6B Q4 / Ollama，`--result-only` | 正确选择工具并返回 84 字符、2 行 | 宿主直接展示回执，不请求模型转述 | 本次单次工具任务完成；不等同于多步 Agent 能力 |

这是一组开发探测，不是固定样本的正式 benchmark。提示和 Schema 在调试中调整过；不能据此计算稳定成功率、比较模型优劣，或宣称低内存真机性能。没有采集本轮完整延迟分布和整机峰值内存。

`--result-only` 的实际回执包含：

```json
{
  "status": "succeeded",
  "output": { "characters": 84, "lines": 2 },
  "completion_recorded": true,
  "continuation_available": true
}
```

省略了每次随机生成的 `plan_id`。`continuation_available` 表示运行时允许继续，不表示示例一定会继续；该模式由宿主选择在这里停止。

## 调试中暴露的问题

早期 MiniCPM 探测直接生成了没有工具依据的错误统计数，促使 SDK 增加可选的 `require_tool_observation` 门禁。该门禁会阻止无观察的最终回答，但不会证明答案语义正确。

Qwen3.5 0.8B 曾返回缺少必需参数的调用，虽然请求中已经包含参数 Schema。宿主类型校验拒绝了候选。Qwen3 0.6B 曾重复请求同一工具；示例限定一次执行，并在预算耗尽后从后续 Schema 移除调用分支，运行时也独立拒绝超额调用。

这些失败保留为当前接入边界。SDK 不使用隐藏的模型重试、字符串修补或确定性答案替换来抬高真实模型结果。

## 复现与下一步

复制 [Ollama 预设](../configs/inference/ollama.yaml)，填入对应模型名及上面的生成设置，然后运行：

```sh
cargo run --locked -p edgehome-inference --example document_agent -- --config path/to/model.yaml --execute-demo
cargo run --locked -p edgehome-inference --example document_agent -- --config path/to/model.yaml --execute-demo --result-only
```

需要定位候选时增加 `--trace-model`；它会输出请求与候选，可能包含业务数据。示例返回 `clarification` 时进程可以正常退出，**退出码 0 不表示任务完成**。评测应分别检查工具选择、参数、执行回执和最终回答。

适合当前版本的接入方式是：少量明确工具、严格的参数与业务校验、宿主确认，以及能够直接消费结构化工具结果的短任务。开放式多步任务需要另建数据集验证。

llama.cpp、LM Studio、vLLM 和 SGLang 本轮只完成本地 HTTP 协议契约测试，未运行它们的原生服务。下一轮应固定模型、提示和任务集，分别验证原生服务兼容性、参数正确率、任务完成率、冷热启动延迟和物理设备内存。
