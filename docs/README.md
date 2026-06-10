# EdgeHome Harness Docs

本目录保存实现计划之外的工程说明文档。

核心文档：

```text
architecture-v2.md   V2 架构定调：Runtime Memory 主路径，Trace/Replay/Eval 观测闭环
2gb-profile.md       2GB RAM 约束、low_memory profile、内存压力降级
2gb-memory-budget.md 2GB RAM 运行时内存预算、模块上限、余量和实测命令
model-parameters.md  MiniCPM5-1B / Ollama 参数、输出治理、调参顺序
deployment-modes.md  Mode A/B/C 部署方式、Home Assistant 接入边界
home-assistant-demo.md  Home Assistant demo 后端、安全边界、token 管理
eval-report-example.md  eval / replay 样例报告和面试表达
demo-walkthrough.md  面试演示脚本顺序、展示点和非目标
small-model-harness-blog.md  对外宣传博客草稿：为什么做小模型 Harness、当前进展和目标
```

注意：

```text
不要在 docs 或 configs 中保存真实 token。
不要把 Home Assistant 讲成项目本体。
不要宣传所有米家设备都能纯离线控制。
```
