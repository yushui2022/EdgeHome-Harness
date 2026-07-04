# Configuration Profiles

本目录存放 EdgeHome Harness 的运行配置、设备注册表和示例后端配置。

不要在本目录提交真实 secret。

## 当前文件

```text
low_memory.yaml                 2GB RAM 主 profile
normal_mode.yaml                开发 / 普通 profile
strict_mode.yaml                更保守的策略 profile
eval_mode.yaml                  eval 使用 profile
demo_mode.yaml                  demo 使用 profile
devices.yaml                    设备注册表与 capability / risk metadata
devices.home_assistant.example.yaml  Home Assistant registry 示例，不含 secret
devices.mqtt.example.yaml       MQTT registry 示例，不含 broker secret
home_assistant.yaml.example     Home Assistant demo backend 示例配置
adapters/mqtt.example.yaml      MQTT dry-run adapter profile 示例；真实 publish 仍需显式执行模式
adapters/miot.example.yaml      MIoT future adapter profile 示例，当前不可运行
```

## low_memory 主线

V2 默认围绕 2GB RAM 约束设计。

核心限制：

```text
num_ctx <= 1024
num_predict <= 128
retry_count <= 1
max_short_memory_turns <= 3
max_context_chars <= 500
executor_backend = mock
dangerous_action_policy = deny
trace_enabled = true
audit_enabled = true
```

内存压力动态决策由代码中的 `ResourcePressurePolicy` 处理。

验证命令：

```powershell
cargo run -q -p edgehome-cli -- config pressure --free-memory-mb 1024
cargo run -q -p edgehome-cli -- config pressure --free-memory-mb 400
cargo run -q -p edgehome-cli -- config pressure --free-memory-mb 128
```

## Device Registry

`devices.yaml` 是执行链路里的业务真相来源之一。

它负责描述：

```text
device_id
room
device_type
aliases
supported actions
capability range
risk level
backend
backend_entity_id
state freshness
```

模型不能直接决定真实设备、风险等级、后端 route 或 `entity_id`。
模型也不能直接输出 MIoT `did / siid / piid`、Matter route 或 MQTT topic。

链路必须是：

```text
模型候选 JSON
-> semantic normalizer
-> device registry
-> capability check
-> policy gate
-> ExecutionPlan
-> executor
```

## Home Assistant 示例配置

`home_assistant.yaml.example` 只作为 demo backend 示例。

`devices.home_assistant.example.yaml` 展示 registry 中的 Home Assistant
`backend_entity_id` 映射。它不是 token 配置，也不代表真实执行默认开启。

真实 token 推荐来自环境变量：

```powershell
$env:EDGEHOME_HA_TOKEN = "your-long-lived-access-token"
```

不要提交：

```text
真实 token
真实账号
真实设备 IP
局域网密钥
私有 token 文件
```

## 配置原则

```text
默认 dry-run / mock。
真实 execute 默认关闭。
高风险策略来自静态配置和设备注册表，不来自 1B 小模型判断。
低内存 profile 不能引入默认重型依赖。
配置变更必须通过 eval --gate。
MQTT adapter profile 当前可用于 dry-run payload；真实 publish 仍未默认开启。
MIoT adapter profile 当前只是 future design example。
```
