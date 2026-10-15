# 推理引擎接入与选择

EdgeHome 将推理服务作为可替换依赖。内置 `HttpProvider` 实现 Ollama 原生协议和 OpenAI-compatible Chat Completions 协议，使用者配置引擎、地址和模型名称即可；业务工具不需要知道推理后端。

## 适配优先级

没有可直接用于本项目的“小模型端侧推理引擎使用率排名”。这里依据官方支持的硬件、部署形式和接口选择适配范围，不把 GitHub 热度或个人偏好当作市场份额。

| 引擎 | 项目中的主要用途 | 配置预设 | 当前验证范围 |
| --- | --- | --- | --- |
| Ollama | 本地开发、快速接入、模型管理 | [ollama.yaml](../configs/inference/ollama.yaml) | 本地 HTTP 协议测试及实际模型探测，结果见下文 |
| llama.cpp | CPU/ARM、GGUF 量化、边缘设备部署 | [llama-cpp.yaml](../configs/inference/llama-cpp.yaml) | 本地 HTTP 契约测试；尚无本轮原生服务或真机验证 |
| LM Studio | 桌面开发、交互式模型加载和本地服务 | [lm-studio.yaml](../configs/inference/lm-studio.yaml) | 本地 HTTP 契约测试 |
| vLLM | GPU/服务器并发推理，可作为边缘应用的远端后端 | [vllm.yaml](../configs/inference/vllm.yaml) | 本地 HTTP 契约测试 |
| SGLang | 高吞吐推理服务，可选服务端后端 | [sglang.yaml](../configs/inference/sglang.yaml) | 本地 HTTP 契约测试 |
| 自定义兼容服务 | 支持本文结构化响应契约的其他服务 | [openai-compatible.yaml](../configs/inference/openai-compatible.yaml) | 同一协议测试，不承诺任意服务都兼容 |

首选开发入口是 Ollama，端侧性能验证重点是 llama.cpp。vLLM/SGLang 的服务端适配不代表它们是低内存设备的默认部署方案。MLX-LM 值得在 Apple Silicon 场景单独验证；本轮未提供专用 MLX/NPU/移动端适配器。

## 请求契约

Ollama 使用 `/api/chat`、`format: <JSON Schema>`、`stream: false` 和 `options.num_predict`。其他预设使用 `<base_url>/chat/completions`，其中 `base_url` 含 `/v1`，通过 `response_format.type: json_schema` 传入 Schema，使用 `max_tokens` 限制生成长度。

这些预设复用两套线协议，不是五套推理内核。候选采用 `call` / `final` / `clarify` JSON 信封；不依赖原生 function-calling 模板或工具解析器。返回原生 `tool_calls`、拒绝响应、截断或缺失内容时会报错，不会悄悄切换到无约束输出。

兼容性还取决于具体服务版本、模型架构、聊天模板和 Schema 子集。推理服务即使接受 `format` 参数，也必须由宿主再校验输出；不能把结构化输出当作权限或语义正确性的保证。

## 配置

复制对应预设，将占位模型名改成服务实际加载的名称。模型准备好后运行文档示例：

```sh
cargo run --locked -p edgehome-inference --example document_agent -- --config configs/inference/llama-cpp.yaml --execute-demo
```

| 参数 | 含义 |
| --- | --- |
| `engine` | `ollama`、`llama_cpp`、`lm_studio`、`vllm`、`sglang` 或 `open_ai_compatible` |
| `base_url` | 可覆盖预设地址；HTTP/HTTPS，拒绝 URL 内嵌凭据、query 和 fragment |
| `model` | 引擎实际服务的名称；SDK 不下载、不猜测模型 |
| `api_key_env` | 可选环境变量名称；令牌不写入配置、错误或 Debug 输出 |
| `connect_timeout_ms` | 建立连接预算，默认 2,000 ms |
| `request_timeout_ms` | 整次请求预算，覆盖等待响应头及读取响应体，默认 15,000 ms |
| `max_response_bytes` | HTTP 响应体上限，默认 65,536 字节，接收过程中落实 |
| `max_output_tokens` | 引擎生成上限，默认 256；不能替代字节限额 |
| `context_tokens` | Ollama 的上下文配置，默认 2,048；其他引擎由服务端配置 |
| `think` | 可选 Ollama 思考开关；支持该选项的模型可设为 false 缩短动作响应，其他引擎配置该字段会报错 |

候选内容还受 `AgentConfig.max_model_output_bytes` 限制。运行时与客户端时限取较短者；示例会把配置中的请求时限同步到运行时。冷启动较慢时显式增加时限或提前加载模型，不自动无限等待。

客户端默认直接连接目标，不使用环境代理，不跟随重定向。响应处理先检查 Content-Length，再有界读取实际字节；没有长度声明和 chunked 响应也受限。错误只包含分类与 HTTP 状态，不携带服务响应正文。关闭请求不会保证所有推理服务立即释放模型计算资源，服务端并发与取消策略仍需配置。

## 验证方法

`cargo test --locked -p edgehome-inference` 使用本地 HTTP 测试服务检查预设路径、参数、Schema、大小限制、错误脱敏、重定向拒绝及请求总时限。它证明客户端协议行为，不模拟真实推理效果。

应用还应分别记录：模型有效候选率、参数正确率、工具选择、澄清率、任务完成率、冷/热启动延迟及整机峰值内存。原有家居 CLI 的 108 条评测不作为通用 SDK 的成功率。

本轮实际 Ollama 探测结果保存在 [SDK 模型验证记录](sdk-model-validation.md)。仅覆盖文档统计任务，不能推广成通用模型排名或生产可用性结论。

## 官方资料

- [Pi](https://github.com/earendil-works/pi)：统一模型 API、可嵌入 Agent 和扩展机制的设计参考。
- [Ollama 结构化输出](https://docs.ollama.com/capabilities/structured-outputs)、[思考开关](https://docs.ollama.com/capabilities/thinking)。
- [llama.cpp](https://github.com/ggml-org/llama.cpp)、[服务接口](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)：量化、硬件后端与本地服务。
- [LM Studio 结构化输出](https://lmstudio.ai/docs/developer/openai-compat/structured-output)。
- [vLLM 服务接口](https://docs.vllm.ai/en/latest/serving/online_serving/openai_compatible_server/)、[支持平台](https://docs.vllm.ai/en/latest/getting_started/installation/)。
- [SGLang 结构化输出](https://docs.sglang.io/docs/advanced_features/structured_outputs)。
- [MLX-LM](https://github.com/ml-explore/mlx-lm)：Apple Silicon 路线的后续验证参考。
