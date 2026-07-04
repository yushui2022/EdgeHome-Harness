# Home Assistant Demo

本文说明 EdgeHome Harness 如何接入 Home Assistant 作为真实设备 demo 后端。

先明确边界：

```text
Home Assistant 是设备后端。
EdgeHome Harness 是项目本体。
模型不直接接触 Home Assistant。
模型不输出 entity_id。
真实 execute 默认关闭。
```

## 当前实现状态

M13 已实现：

```text
HomeAssistantConfig
SecretsLoader
HomeAssistantClient
HomeAssistantExecutor
HA state fetch
HA dry-run service translation
HA service call translation
```

代码位置：

```text
crates/edgehome-executor/src/home_assistant.rs
```

配置样例：

```text
configs/home_assistant.yaml.example
```

## Token 管理

不要把 Home Assistant token 写进：

```text
README
docs
configs
prompt
trace 普通字段
audit 普通字段
```

推荐用环境变量：

```powershell
$env:EDGEHOME_HA_TOKEN = "your-long-lived-access-token"
```

或在私有路径使用 token 文件：

```yaml
token_file: C:\Users\you\.edgehome\ha-token.txt
```

不要把 token 文件放进仓库。

## Registry 映射

模型只看到内部设备候选摘要。

真实 HA 映射由 Device Registry / Executor 完成。

示例：

```yaml
device_id: hallway_light
backend: home_assistant
backend_entity_id: light.hallway
```

模型输出不能包含：

```text
light.hallway
Home Assistant token
Home Assistant API path
backend route
```

## Dry-run 翻译

用户输入：

```text
晚上十点后把走廊灯调到30%
```

Harness 标准化为内部命令：

```json
{
  "device_id": "hallway_light",
  "device_type": "light",
  "action": "set_brightness",
  "params": {
    "brightness": 30,
    "time_after": "22:00"
  }
}
```

HA dry-run 结果应该包含：

```json
{
  "backend": "home_assistant",
  "entity_id": "light.hallway",
  "service": "light.turn_on",
  "service_path": "/api/services/light/turn_on",
  "payload": {
    "entity_id": "light.hallway",
    "brightness_pct": 30
  }
}
```

这一步仍然不是执行。

它只是把 `ExecutionPlan` 翻译成将要调用的 HA service。

## Execute 边界

真实执行必须满足：

```text
execute_enabled = true
ExecutionPlan 已生成
PolicyDecision != deny
配置为 require_confirmation 的动作已经用户确认
ExecutionTransaction 通过
RateLimiter 通过
IdempotencyChecker 通过
PostStateVerifier 通过
```

默认配置：

```yaml
execute_enabled: false
```

因此未显式开启时，即使调用 executor，也会返回：

```text
real execution is disabled by default
```

## 不要夸大 Xiaomi 离线能力

可以说：

```text
EdgeHome Harness 可以通过 Home Assistant 接入已有智能家居设备。
具体设备是否离线，取决于 Home Assistant 集成和设备类型。
```

不要说：

```text
所有米家设备都能被本项目纯离线控制。
本项目替代小米音箱。
本项目替代米家 App。
本项目替代 Home Assistant。
```

## 推荐 demo 路线

面试 demo 不建议直接真实开关设备。

推荐顺序：

```text
1. 先跑 eval，展示 Harness 指标
2. 跑 dry-run，展示 ExecutionPlan
3. replay trace，展示 trace / policy / audit
4. 展示 policy-configured deny 样例
5. 展示短时记忆解析“刚才那个灯”
6. 最后展示 HA dry-run service payload
```

真实设备 execute 只作为可选加分项，并且必须手动确认。

## 测试覆盖

当前单测覆盖：

```text
example_config_loads_with_execution_disabled
missing_token_does_not_panic
secret_debug_is_redacted
translates_light_brightness_to_service_call
translates_climate_temperature_to_service_call
rejects_entity_id_with_path_injection
executor_execute_is_disabled_by_default
executor_uses_route_to_translate_plan
executor_rejects_non_home_assistant_dry_run_plan
dry_run_planner_translates_home_assistant_payload
```

验证命令：

```powershell
$env:CARGO_TARGET_DIR="$env:TEMP\edgehome-target"
cargo test -p edgehome-executor
```
