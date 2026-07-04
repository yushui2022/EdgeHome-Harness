# QEMU 2GB 嵌入式预验证与真实模型测试报告

Date: 2026-06-12

本报告记录 EdgeHome Harness 在 2GB ARM64 模拟嵌入式环境中的**预验证部署、真实小模型问答测试、按模块内存占比、调试方法与可达效果**。

> 性质说明:这是一个**真机前的预验证环境**(QEMU TCG 软件模拟 ARM64),不是树莓派真机性能基准。所有延迟数字仅在"同一环境内做相对比较"时有效,不能当作真机 token/s。

---

## 1. 验证目标

预验证在严格 2GB 内存约束的 QEMU ARM64 Linux 上,可以同时运行:

- **Ollama 0.30.7 (arm64) + MiniCPM5 q4 小模型**(本地推理)
- **EdgeHome Harness(13-crate Rust 工程)**(把"模型候选输出"安全收敛为"可执行计划")

并验证核心论点 **`ModelOutput != Command`**:小模型在边缘设备上输出不可靠,Harness 必须把不可靠输出**安全降级为拒绝执行**,而不是误操作用户设备。

---

## 2. 部署架构(多层虚拟化)

```text
Windows 11 (x86)
 └─ WSL2 Ubuntu (Ubuntu-24.04-EdgeHome, E 盘)        ← 控制面 / 快网下载
     └─ QEMU virt ARM64 (-m 2048, cortex-a72, smp 2) ← 2GB 模拟嵌入式 guest
         ├─ Ollama 0.30.7 (arm64) + MiniCPM5 q4 (688MB blob)
         └─ edgehome Rust harness (aarch64 二进制, 4.9MB)
```

Guest 基线(实测):

```text
arch: aarch64
MemTotal: 2002264 kB (~1.9 GiB)
Swap: 0B
cloud-init: done
SSH: 经 hostfwd 2222 + 私钥 chmod 600 可达
```

---

## 3. 部署链路踩坑与可复用解法

多层虚拟化(Windows → WSL → QEMU)的命令转义/路径/后台/网络问题是本次主要工程成本。下表为**可直接照搬的解法**:

| 问题现象 | 根因 | 可复用解法 |
|---|---|---|
| 脚本变量经 `bash -lc`/wsl.exe 被吃掉 | 多层 shell 转义 | **整脚本 base64 编码传输**,落地 `base64 -d` 还原 |
| `/mnt` 路径被改写成 `D:/Git/mnt` | Git Bash MSYS 路径转换 | 命令前加 `MSYS_NO_PATHCONV=1` |
| 后台进程随 SSH/WSL 关闭被杀 | nohup/Start-Process 跨层不可靠 | `setsid bash -c "..." < /dev/null &` |
| 后台 stdout 永远为空 | 多层 SSH 缓冲 | 不读 stdout,**直接读 guest 侧 artifact 文件** |
| guest 内 curl 拉 1.5G ollama 卡 10 分钟 | guest NAT 出网慢 | 在 WSL 宿主快网下载,再 `scp` 进 guest |
| ollama 资产 404 | 资产名错 | 认准 `ollama-linux-arm64.tar.zst`(不是 `.tgz`) |
| 真实 eval 首调 `os error 11` | 模型未加载,连接早断 | **先 `ollama run ... --keepalive` 预热模型**再跑 harness |
| `unknown profile low_memory_ollama` | profile 是硬编码枚举非文件名 | 调参直接改 `configs/low_memory.yaml` 本身 |
| `model 'openbmb/minicpm5:1b' not found` 404 | config tag 与实际拉取 tag 不符 | 对齐 `model_name` 与 `ollama list` 的 tag |

**方法论结论:不要相信文档/记忆里"已完成"的状态,一律实测 guest 当前真实状态。** 旧报告写"blocked on Rust toolchain",实际所有 blocker 早已解决。

---

## 4. 2GB 下的按模块内存占比(实测)

通过三态采样(A=模型未加载 / B=模型常驻 / C=模型常驻+harness 并发 eval),`/proc/meminfo` + `ps RSS` 实测:

| 模块 | RSS / 成本 | 说明 |
|---|---|---|
| **llama-server**(权重+KV+buffer) | **827,996 kB ≈ 828 MB** | 模型常驻主体,内存大头 |
| **ollama serve+runner** 控制面 | 38,032 kB ≈ 38 MB | 进程管理/HTTP API |
| **edgehome harness**(eval 中) | **5,744 kB ≈ 5.7 MB** | Rust 极轻 |
| 模型加载成本 (A−B) | 804,916 kB ≈ 805 MB | 加载前后 MemAvailable 差 |
| MemAvailable 空闲 (A) | 1,735,556 kB ≈ 1.74 GB | |
| **MemAvailable 模型常驻 (B)** | **930,640 kB ≈ 930 MB** | **常驻后仍剩近 1GB** |
| MemAvailable 常驻+harness (C) | 934,340 kB | harness 几乎不增内存 |

磁盘占用:

```text
/usr/bin/ollama   : 34 MB
/usr/lib/ollama   : 2.1 GB   (含 runner/库,可裁剪)
MiniCPM5 q4 blob  : 688 MB
edgehome 二进制    : 4.9 MB
```

**关键结论:STATE C 下 `gate.passed: true`,OOM 事件 = 0,Swap = 0。** 在这次 QEMU 预验证环境中,模型常驻 + harness 并发运行可以落在 2GB 预算内,还剩约 930MB 余量。瓶颈是 llama-server 的 828MB,而非 harness(仅 5.7MB)。这不是真实 2GB ARM 板卡的长跑性能结论。

---

## 5. 单条真实模型问答测试

向真实 MiniCPM5 输入中文 IoT 指令「**把客厅灯关掉**」,经完整 Harness 流水线(guard→context→model→parser→memory→gate→dry-run),实测 `trace_id=tr_18b83f28e824a0c5_2`:

### 5.1 模型原始输出 —— 看似干净,实则缺槽位

```json
{"action": "turn_off", "room": "living_room"}
```

仅 45 字节。模型给出了 `action`/`room`,但**漏掉了 `intent`、`device_type`、`device_id`**。这正是 `ModelOutput != Command` 要拦的"看起来对、其实不能执行"的输出。

### 5.2 Harness 九道 Gate 逐层拦截

| Gate | 结果 | 原因 |
|---|---|---|
| SchemaGate | rejected (blocking) | intent is unknown |
| DeviceResolvedGate | rejected (blocking) | 无 resolved device_id |
| CapabilityGate | rejected (blocking) | 无 device_id 无法校验能力 |
| FreshnessGate | accepted | 设备状态新鲜 |
| **PolicyGate** | **rejected (blocking)** | **policy denies risk `Unknown`** |
| ConfirmationGate | warning | 确认无法覆盖被拒策略 |
| DryRunGate | warning | 策略拒绝则跳过试运行 |
| ExecutionGate | accepted | 未请求执行 |
| MemoryWriteGate | accepted | 无记忆写入 |

最终:`policy_decision: deny`,`executable: false`,`execute_enabled: false`。

### 5.3 可审计的证据链

```text
evidence_refs:
  raw_user_input   ev_18b83f28e47a057e_1   "把客厅灯关掉"
  raw_model_output ev_18b83f416a95e374_4   {"action":"turn_off","room":"living_room"}
  parsed_json      ev_18b83f4173f75f84_6
  normalized_command ev_18b83f417c16ee76_8
```

**意义:小模型补不全关灯指令的关键槽位,Harness 没有放行,而是把它安全降级为"拒绝执行"——用户家里的灯不会被误操作。整条链路留有 evidence + trace,可审计、可回溯。这就是项目存在的理由。**

---

## 6. AI 稳定性观测

1. **首 token 延迟高**:本次 `latency_ms = 106263`(106 秒)——TCG 软件模拟 ARM 所致,**非真机基准**;真实 ARM 板会快得多。
2. **加载即跑题**:模型预热时出现自我对话式 rambling("I should keep my response friendly...")——典型小模型跑题倾向,正是 Output Governor 存在的意义。
3. **Output Governor 本次状态**:`accepted:true`,`raw_bytes:45`,`repeat_detected:false`,未触发降级(输出短)。但 governor 已就位,长输出/死循环场景会拦截。
4. **槽位缺失是常态**:连最简单的关灯指令都补不全 intent/device_id,佐证 Harness 的强校验是必要的而非冗余。

---

## 7. 调试与调参方法

### 7.1 真实模型 eval 的正确姿势(避坑顺序)

```bash
# 1) 先预热模型并保活,否则 harness 首调必报 os error 11
echo OK | ollama run openbmb/minicpm5:latest --keepalive 30m

# 2) 模拟环境延迟极高,需放宽 harness 超时(改 yaml 本身,profile 是硬编码枚举)
sed -i 's/timeout_ms: 8000/timeout_ms: 120000/' configs/low_memory.yaml

# 3) 对齐模型 tag(config 的 model_name 必须等于 ollama list 的 tag)
grep model_name configs/low_memory.yaml      # openbmb/minicpm5:latest

# 4) 单条真实解析探针(看完整 trace_frame)
./edgehome --profile low_memory --config-dir configs \
  --db-path artifacts/probe.sqlite parse "把客厅灯关掉"
```

> 注:`timeout_ms=120000` 仅为绕过 TCG 模拟的超长延迟;**真机上应保留默认 8000ms**,否则失去 governor 超时保护的意义。

### 7.2 三档内存压力调参(low_memory profile 设计)

| 档位 | num_ctx | num_predict | memory | 触发 |
|---|---|---|---|---|
| normal | 1024 | 128 | on | 内存充裕 |
| elevated | 768 | 96 | on | 内存偏紧 |
| critical | 512 | 64 | off / rule_only | 内存告急 |

收紧 `num_ctx`/`num_predict` 直接降低 llama-server 的 KV cache 占用——这是在 2GB 内给 harness 腾空间的主要旋钮。

### 7.3 内存占比采样方法

```bash
# 三态对比:A 未加载 / B 常驻 / C 常驻+harness
grep -E 'MemTotal|MemFree|MemAvailable|Cached' /proc/meminfo
ps -eo rss,comm --sort=-rss | head -6
# 模型加载成本 = MemAvailable(A) - MemAvailable(B)
```

---

## 8. 可达效果(结论)

**本次 QEMU 预验证已覆盖:**

- QEMU 2GB 模拟嵌入式环境可**同时**承载 ollama + MiniCPM5 + Rust harness,常驻后剩 ~930MB,**OOM=0 / Swap=0**。
- 单条真实模型 → Harness 全链路打通:Gate / Trace / Evidence / Policy **真实生效**。
- `ModelOutput != Command` 被单条真实劣质输出验证:模型漏槽位 → Harness deny → 不误操作设备。
- 本次环境内的 mock eval gate 与真实模型并发 eval 均 `passed: true`。

**未证明 / 需注意:**

- 延迟(106s)是 TCG 模拟值,**不能当真机性能**,需真 ARM 板复测。
- 默认 8s 超时在模拟环境不适用,只有真机才有评估意义。
- `/usr/lib/ollama` 2.1GB 磁盘可进一步裁剪。
- 尚未证明真实 2GB ARM 板卡上的长期稳定性、真实 token/s 或多轮负载表现。

**下一步:**

1. 真 ARM 板(树莓派/RK35xx)上复测真实 token/s 与三档压力下的降级行为。
2. 真实模型下系统化跑 `eval cases/zh-home.yaml --ollama` 全量用例,统计 intent/slot/policy 准确率。
3. 裁剪 ollama runner 库,压缩磁盘占用。

---

## 9. 关键工件

```text
artifacts/mem-final.txt        三态内存占比原始数据
artifacts/real-probe2.json     真实模型「把客厅灯关掉」完整 trace
artifacts/eval-gate.json       mock low_memory eval gate (passed:true)
```
