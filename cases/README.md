# Evaluation Cases

本目录存放 EdgeHome Harness 的评测用例。

当前主用例文件：

```text
zh-home.yaml
```

这些 case 的目标不是测试开放聊天能力，而是测试 1B 端侧小模型在智能家居窄场景下是否能被 Harness 稳定约束。

当前覆盖：

```text
普通关灯
短时相对指令
时间 + 亮度槽位
明确长期别名写入
长期别名解析
空调温度
空调开关
门锁策略样例
摄像头策略样例
燃气报警器拒绝样例
```

评测指标包括：

```text
intent_accuracy
slot_accuracy
policy_accuracy
dry_run_accuracy
trace_coverage
schema_valid_rate
memory_resolution_accuracy
fallback_rate
dead_loop_rate
retry_rate
latency_avg_ms
latency_p95_ms
low_memory_degrade_count
```

运行命令：

```powershell
$env:CARGO_TARGET_DIR="$env:TEMP\edgehome-target"
cargo run -q -p edgehome-cli -- --profile low_memory --db-path edgehome-eval.sqlite eval cases/zh-home.yaml
cargo run -q -p edgehome-cli -- --profile low_memory --db-path edgehome-gate.sqlite eval cases/zh-home.yaml --gate
```

注意：

```text
默认 eval 使用 mock model，不依赖真实设备。
--gate 用于 release quality gate，不是普通智能家居动作门禁。
新增 case 时必须能落到 trace/replay，否则不能证明 Harness 链路可观测。
```
