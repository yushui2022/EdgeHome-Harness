# EdgeHome Harness Docs

本目录保存实现计划之外的工程说明文档。

核心文档：

```text
architecture-v2.md   V2 架构定调：Runtime Memory 主路径，Trace/Replay/Eval 观测闭环
command-pipeline-contract.md  模型候选 JSON、内部命令、ExecutionPlan、BackendAdapter 的边界
backend-adapter-contract.md  Mock / Home Assistant / MQTT dry-run / future adapter 的实现契约和 fail-closed 规则
customization.md  用户如何定制设备、capability 和后端映射；模型输出 schema 为什么固定
roadmap.md  当前 baseline、near-term work、adapter 顺序、硬件证据要求和 non-goals
release-checklist.md  release 前必须跑的检查、gate 阈值、docs 审查和 secrets 审查
2gb-profile.md       2GB RAM 约束、low_memory profile、内存压力降级
2gb-memory-budget.md 2GB RAM 运行时内存预算、模块上限、余量和实测命令
model-parameters.md  MiniCPM5-1B / Ollama 参数、输出治理、调参顺序
deployment-modes.md  Mode A/B/C 部署方式、Home Assistant 接入边界
home-assistant-demo.md  Home Assistant demo 后端、安全边界、token 管理
home-assistant-gateway.md  Home Assistant gateway boundary、真实执行开关、状态回读
home-assistant-golden-payloads.md  Home Assistant dry-run golden payloads 和 claim boundary
mqtt-guarded-publish.md  MQTT dry-run payload 与 guarded publish executor
miot-bridge-adapter.md  MIoT/Xiaomi bridge request adapter 与真实设备验证边界
matter-bridge-adapter.md  Matter controller bridge adapter 与非目标
eval-report-example.md  eval / replay 样例报告和面试表达
real-minicpm-eval-report.md  真实 MiniCPM/Ollama eval report 工作流和报告模板
demo-walkthrough.md  面试演示脚本顺序、展示点和非目标
small-model-harness-blog.md  对外宣传博客草稿：为什么做小模型 Harness、当前进展和目标
waic-one-page.md  对外 one-page 口径：定位、证据、当前边界和未来 adapter 方向
```

注意：

```text
不要在 docs 或 configs 中保存真实 token。
不要把 Home Assistant 讲成项目本体。
不要宣传所有米家设备都能纯离线控制。
不要把 MIoT / Matter bridge adapter 写成全量生态支持。
不要把 MQTT guarded publish 写成默认真实 broker publish。
```
