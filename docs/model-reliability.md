# 真实模型可靠性验证

这组验证衡量短任务能否选对工具、提取正确参数并取得正确执行回执，以及缺少信息时能否提出相关问题。它不把合法 JSON、mock 通过率或测试数量当作模型可靠性的替代指标。

后续已加入可恢复澄清、应用业务校验，以及使用真实文件的多轮对照，见
[文件 Agent 验证](file-agent-results.md)。本页原始 36 题与记录保持不变，后续作为回归依据；
新的文件任务和本页的单步任务不合并计算准确率。

## 本次结果

实验于 2026-10-15 UTC 运行，使用 Ollama 0.18.3。36 个本地验收用例各重复 3 次；表中的分母 108 是试验次数，不是 108 个独立问题。另有 8 题开发集的配对重复，总计 792 次任务试验。

| 模型 | 原协议自动通过 | action 自动通过 | action 澄清语义复核后 | 错误提案：原 / action | 验收 |
| --- | --- | --- | --- | --- | --- |
| Qwen3 0.6B Q4，修正聊天模板 | 36/108 | 82/108 | 79/108 | 0 / 17 | 未通过 |
| Qwen3.5 0.8B | 24/108 | 75/108 | 69/108 | 0 / 18 | 未通过 |
| MiniCPM5 1.1B Q4 | 66/108 | 78/108 | 78/108 | 21 / 27 | 未通过；system 探针也失败 |

新协议提高了工具任务完成率，同时产生了更多通过类型校验的错误提案。**这些模型配置尚不适合预授权的无人值守执行。** action 保持显式选择，默认协议和逐计划确认机制保留；不能把更少的格式错误当作更可靠的业务判断。

[完整记录](validation/reliability-2026-10-15/manifest.json)保留六组协议/模型配置的开发与验收报告，包括全部失败、原始响应或错误、回执、预算、模型 digest 和源码指纹。模板、token 上限、上下文和模型保持同模型内一致，因此表中协议差异不混入 Qwen 模板修复收益。

初始自动规则仍会误收不合格的澄清。例如，Qwen3.5 把不支持的 `report` 列成可选文档，另一个回复又要求用户重述已经提供的 `2 inches`。本次助手逐条复核了所有自动通过的验收澄清，仅扣除失败、不提升失败样本，结果见[澄清审计](validation/reliability-2026-10-15/clarification-audit.json)。这属于运行后的定性复核，**不是第三方盲评**。复算器检查审计覆盖及数字一致性，不能自动证明语义判断正确。

主要缺口集中在歧义、否定/改口、不支持的对象，以及缺失参数时猜值。单纯调整 JSON 协议不能解决这些问题；当前报告没有宣称通用语义可靠性已达标。

补充核对了 MiniCPM 的[官方聊天模板](https://huggingface.co/openbmb/MiniCPM5-1B/blob/main/chat_template.jinja)，并以单独本地别名测试显式非思考前缀。该变体仍未通过 system 探针，因此没有作为修复方案，也没有替换主实验数据；[补充记录](validation/reliability-2026-10-15/minicpm-prefix-followup.json)单独保留。

## 可用机制

- `runtime.decision_protocol: action`：先生成动作名称，再生成参数。系统提示中的 schema 与生成约束来自同一份工具定义，避免复制占位示例和把枚举值当作默认参数。
- 对明确非空的顶层必填字段，模型可以输出 `null` 表示缺失。宿主将它转换为询问，不创建可执行计划；工具本身的类型与校验不放宽。复杂引用和本来允许 null 的字段保持原契约。
- `runtime.max_decision_repairs: 1`：最多增加一次格式或参数校验纠错，计入 `max_model_calls`。有效但错误的决策不重试；权限拒绝、网络错误、超时、超长响应和工具执行不重试。没有标准答案参与纠错。
- `doctor --probe`：分别从 system 消息读取两个不同的测试值，答案不出现在 user 消息或响应 schema 中。每次请求都有现有时间与字节限制。通过仅证明这个基本指令测试，不证明任务准确率。
- 保留默认 `tagged` 协议以兼容已有 SDK 调用方；新配置位于 [configs/reliability](../configs/reliability/)。

本机导入的 `mayulu/qwen3-0.6B-Q4_K_M:latest` 使用 `{{ .Prompt }}` 模板且没有 renderer，未通过 system 值测试。为对照建立了单独的 `edgehome-qwen3-06b-chat:v1` 别名，复用原权重并使用 ChatML 对话模板与非思考前缀。没有改写原模型或下载权重。`qwen3.5:0.8b` 的模板字段也显示 `{{ .Prompt }}`，但它有内置 renderer，因此不能只凭模板字符串判定模型坏了。

模板实现依据 [Qwen 官方 tokenizer 配置](https://huggingface.co/Qwen/Qwen3-0.6B/blob/main/tokenizer_config.json)；别名创建方法见 [Ollama Modelfile 文档](https://docs.ollama.com/modelfile)。将 schema 同时提供给提示词和生成参数，参考 [Ollama structured outputs 文档](https://docs.ollama.com/capabilities/structured-outputs)。

## 复现

先准备配置中指定的本地模型。修复上述特定 Qwen3 导入模型时可运行：

```sh
ollama create edgehome-qwen3-06b-chat:v1 -f configs/ollama-qwen3-chat.Modelfile
cargo run -p edgehome-kit --example reliability_lab -- doctor --config configs/reliability/qwen06-action.yaml --probe
```

此模板仅适用于所列 Qwen3 文本模型，不应应用到 Qwen3.5、MiniCPM 或其他模型族。

```sh
cargo run -p edgehome-kit --example reliability_lab -- eval \
  --config configs/reliability/qwen06-action.yaml \
  --cases cases/sdk-reliability-holdout.json \
  --repeat 3 --order-seed 42 --include-responses \
  --allow document.stats --allow sensor.read --allow length.convert
```

完整配对实验及独立复算：

```sh
python scripts/evaluate-sdk-reliability.py --output /path/to/new-evidence-directory
python scripts/verify-sdk-evidence.py /path/to/new-evidence-directory
```

证据复算通过只说明记录和评分一致。上线前的模型验收可追加 `--require-gate qwen06-action-holdout`；该模型未达到预定门槛时返回非零，不能把“报告校验通过”写成“模型验收通过”。`--self-test` 会在临时副本中故意改分、删题并重算文件哈希，确认独立复算仍能发现这些问题。

Windows 本地将输出放在 `G:\\DevCache\\Temp\\edgehome`，并设置 `CARGO_HOME`、`CARGO_TARGET_DIR`、`TEMP` 指向 G 盘。完整实验要求依赖已缓存，构建使用 `--offline --locked`；不会自动拉取模型。输出目录必须不存在。脚本保留失败，任务评测遇到任一失败返回非零；完整实验脚本负责收集全部对照，质量门槛记录于 manifest。

## 实验约定

| 项目 | 约定 |
| --- | --- |
| 开发集 | 8 个既有入门任务，澄清题增加问题内容检查 |
| 验收集 | 36 个未用于调试结果的本地用例：文档 8、传感器 8、长度换算 8、缺参/歧义/不支持 12 |
| 路由 | 验收集每题同时暴露三个工具，不用答案为模型选择唯一工具 |
| 重复 | 每个模型、协议、用例运行 3 次，每次新 Agent；固定 seed 42 只打乱执行顺序 |
| 生成 | 温度 0、上下文 4096、输出上限 1024 token、单次请求 60 秒；每任务最多 2 次模型调用、1 次工具调用 |
| 对照 | 同一模型的 tagged/0 次纠错与 action/1 次纠错；模型模板与其他配置相同 |
| 工具 | 文档和传感器为固定测试数据；长度换算是真实的纯计算；全部无外部副作用 |
| 执行 | 所有获 `--allow` 授权的提案实际执行，再与答案比对；答案不会参与模型提示、运行时纠错或执行放行 |
| 通过 | 正确工具、正确参数、成功且记录完成的回执、正确输出；澄清必须进入澄清分支且含预期问题主题 |
| 数值 | 2 与 2.0 可视为同值；不强制转换字符串，不用宽松误差掩盖错误 |
| 门槛 | 每轮成功率 ≥90%、每类别成功率 ≥90%、错误提案为 0，且 system 探针通过 |
| 证据 | 原始响应、错误、执行回执、每轮/类别分数、首轮成功、纠错恢复、延迟、模型 digest、模板、参数、代码与二进制哈希 |

两个协议的对照是整体协议/提示策略对照，不是严格拆分每个组成部分的消融实验。模板修复、提示压缩、参数提取和纠错各自的独立收益不能由这组结果推导。

## 如何解读结果

- 同一用例在温度 0 下重复三次衡量一致性，不等于三个独立样本；不据此宣称统计显著性，也不把 108 次调用称为 108 个独立任务。
- 用例由本项目作者编写，且集中在三个简单工具上；是开发阶段的固定验收集，不是第三方盲测或跨行业能力证明。
- 澄清内容的关键词检查是有限的自动规则，仍需检查原文是否合理；它不能完整证明自然语言语义。
- 记录的延迟包含本地请求和工具回执；不是受控硬件性能基准，不包含端侧功耗、RSS 或长时间热稳定性测量。
- 测量终点为一次工具回执或澄清，不覆盖最终自然语言总结、多步规划、真实设备 I/O、恢复或无人值守操作。
- 有效但错误的提案仍可能通过类型校验，因此应用仍需要自己的权限、状态校验及确认。schema 不能替代业务正确性。

原始响应仅在显式 `--include-responses` 时输出，可能包含输入数据的复述；传输层已拒绝的截断或超长响应只记录错误分类，不保存被丢弃的正文。正式应用默认的 metadata journal 仍不记录这些内容。复算器核对文件哈希、逐题记录、原始回执、各项汇总与门槛；`--skip-source-check` 仅用于历史源码已变化的归档，不能跳过报告和答案验证。
