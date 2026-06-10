# 2GB RAM Memory Budget

本文说明 EdgeHome Harness 在 2GB RAM 级边缘设备上的运行时内存预算。

核心结论：

```text
2GB 指的是 RAM，不是 microSD / eMMC / NVMe 存储。
MiniCPM5 q4_K_M 的 688MB 是模型文件体积，不等于完整运行时 RAM 占用。
2GB 可以作为 low_memory profile 的极限验证目标，但不应该按舒适部署环境设计。
4GB 才是 MiniCPM5-1B + Harness 的实际推荐部署起点。
```

## RAM 和存储不要混淆

树莓派 5 的 2GB / 4GB / 8GB 指的是 LPDDR4X RAM。官方规格列出 Raspberry Pi 5 支持 1GB、2GB、4GB、8GB、16GB LPDDR4X 版本。

存储是另一回事：

```text
microSD / eMMC / NVMe = 保存系统、模型文件、日志、SQLite 数据库
RAM = 运行时加载模型、KV cache、推理 buffer、进程堆栈、系统缓存
```

MiniCPM5 在 Ollama 上的 q4_K_M / latest 模型大小是 688MB，q8_0 是 1.2GB，fp16 是 2.2GB。这个数字主要描述模型文件或模型包大小。

运行时还会额外占用：

```text
模型后端进程内存
模型 mmap / page cache
KV cache
推理临时 buffer
Ollama runtime 开销
Rust Harness 进程内存
SQLite page cache
日志缓冲
系统服务
内核 / CMA / GPU / 网络栈
```

因此不能用：

```text
2GB RAM - 688MB 模型 = 还剩 1.3GB，所以很宽裕
```

这个算法是错的。

## 2GB 机器上真正可用的预算

2GB 物理内存并不等于应用可随意使用 2048MB。

Linux 中 `/proc/meminfo` 的 `MemTotal` 是物理 RAM 减去内核等保留区域后的可用总量；`MemAvailable` 是 Linux 对“不发生 swap 时还能启动多少新应用”的估计值。

对 2GB 树莓派，工程预算不要按 2048MB 算，建议按下面思路设计：

```text
物理 RAM：2048MB
实际可调度总量：约 1850-1950MB，视系统、内核、CMA、GPU、启动项而定
安全设计预算：约 1700-1800MB
必须预留余量：至少 250-350MB MemAvailable
```

如果长期运行时 `MemAvailable` 经常低于 200MB，系统已经进入危险区。

## 推荐内存预算：2GB + llama.cpp 路线

如果坚持 2GB，优先推荐更可控的 llama.cpp / GGUF 路线，而不是把 Ollama 作为长稳主路径。

目标预算：

| 模块 | 目标占用 | 上限 | 说明 |
|---|---:|---:|---|
| OS Lite + kernel + systemd + SSH + network | 280-350MB | 450MB | 必须使用无桌面系统 |
| llama.cpp server + MiniCPM5 q4 | 850-1050MB | 1150MB | q4，短上下文，单并发 |
| KV cache / context / inference buffer | 已包含在模型后端预算内 | 额外不得超过 150MB | `num_ctx` 必须限制 |
| EdgeHome Harness Rust binary | 25-64MB | 96MB | 目标是常驻小进程 |
| SQLite / trace / audit buffer | 5-32MB | 64MB | trace 落盘，不长期堆内存 |
| MQTT / HTTP / demo executor | 10-32MB | 64MB | 不同执行器差异较大 |
| 临时对象 / page cache / burst | 80-150MB | 200MB | 给 JSON、日志、请求峰值留空间 |
| 安全余量 `MemAvailable` | 300-400MB | 最低 200MB | 低于阈值必须降级 |

目标状态：

```text
常态总占用控制在 1500-1650MB
MemAvailable 保持 300MB 以上
不依赖磁盘 swap 才能正常响应
```

这条路线最适合验证：

```text
2GB low_memory profile
短上下文
短输出
单并发
rule-only fallback
内存压力降级
```

## 谨慎预算：2GB + Ollama 路线

2GB 上不是不能跑 Ollama，但应视为实验路线。

预算会更紧：

| 模块 | 目标占用 | 上限 | 说明 |
|---|---:|---:|---|
| OS Lite + kernel + systemd + SSH + network | 280-350MB | 450MB | 禁止桌面环境 |
| Ollama daemon + loaded MiniCPM5 q4 | 1000-1250MB | 1350MB | 与版本、mmap、缓存、上下文相关 |
| EdgeHome Harness Rust binary | 25-64MB | 96MB | 需要限制内部缓存 |
| SQLite / trace / audit buffer | 5-32MB | 64MB | trace 必须流式落盘 |
| MQTT / HTTP / demo executor | 10-32MB | 64MB | 不建议同机跑重服务 |
| 临时对象 / page cache / burst | 80-150MB | 200MB | 留给请求峰值 |
| 安全余量 `MemAvailable` | 250-350MB | 最低 180MB | 低于阈值不再发起 LLM 调用 |

如果 Ollama + 模型实际常驻超过 1.3GB，2GB 机器就不适合继续走 Ollama 长稳路线。

此时应该切换到：

```text
llama.cpp provider
或升级到 4GB RAM
```

## 不建议同机运行的东西

2GB 模式下，不建议同机运行：

```text
桌面环境
浏览器
完整 Home Assistant Core
数据库服务端
多个模型
q8 / fp16 模型
ASR / TTS / 视觉模型
多并发推理
大量 debug 日志
长时间 benchmark
```

Home Assistant 可以作为外部 backend。2GB 板子上的 EdgeHome Harness 只保留 demo executor / bridge，不应该把完整 HA 全家桶也塞进同一台 2GB 板子。

## EdgeHome Harness 的 low_memory profile 建议

2GB 模式下，模型参数应该保守：

```text
model = openbmb/minicpm5:q4_K_M
temperature = 0.1 ~ 0.3
top_p = 0.7 ~ 0.9
top_k = 20
repeat_penalty = 1.2 ~ 1.3
enable_thinking = false
num_ctx = 512 / 768 / 1024
num_predict = 64 / 96 / 128
concurrency = 1
```

记忆注入建议：

```text
short_memory_max_turns = 3
long_memory_injection_max_items = 3
context_summary_max_chars = 300-500
failure_audit_not_in_prompt = true
low_memory_disable_long_memory = true
```

trace 建议：

```text
raw_model_output 限长
trace 立即落盘
不要把完整 trace 长期留在内存 Vec 中
eval 和 replay 不在 2GB 常驻路径中运行
```

## 内存压力分级

推荐用 `MemAvailable` 作为主阈值。

| 状态 | `MemAvailable` | 行为 |
|---|---:|---|
| healthy | >= 450MB | 正常 LLM JSON，短时记忆开启，长期偏好最多注入 3 条 |
| constrained | 300-450MB | compact JSON，降低 `num_ctx` / `num_predict`，限制长期记忆 |
| critical | 180-300MB | 停止长期记忆注入，只允许短输出；优先 rule parser |
| emergency | < 180MB | 不再发起新 LLM 调用，返回 fallback / safe reject，必要时卸载或重启模型进程 |

更保守的生产口径：

```text
MemAvailable < 350MB：进入低内存模式
MemAvailable < 250MB：不再允许重试
MemAvailable < 200MB：不再发起 LLM 调用
MemAvailable < 150MB：清理短时缓存、flush trace、准备重启模型后端
```

## 各模块应该如何分配内存责任

### 模型后端

模型后端是最大头。

约束：

```text
只用 q4_K_M
禁止 q8 / fp16
num_ctx 默认 512 或 1024
num_predict 默认 64 或 128
单并发
禁用 thinking
```

目标：

```text
2GB + llama.cpp：模型后端常驻 <= 1050MB，硬上限 1150MB
2GB + Ollama：模型后端常驻 <= 1250MB，硬上限 1350MB
```

### Rust Harness

Harness 不能把自己写成第二个大内存服务。

约束：

```text
常驻 RSS 目标 25-64MB
硬上限 96MB
不在内存里堆完整 trace
不把长期记忆全量加载
不保存完整自然语言聊天历史
配置和 registry 小型化
```

### Memory Manager

记忆系统必须结构化。

约束：

```text
短时记忆只保存 last_target / last_action / last_value 等结构化状态
长期记忆存 SQLite
每轮最多注入 3 条相关偏好
低内存时关闭长期记忆注入
失败审计不进 prompt
```

预算：

```text
常驻内存 <= 10MB
上下文摘要 <= 300-500 中文字符
```

### Trace / Audit

Trace 的价值是可回放，不是常驻内存。

约束：

```text
流式写 SQLite
raw output 限长
每次请求结束后释放临时对象
eval / replay 离线跑，不在 2GB 在线路径常驻
```

预算：

```text
在线 trace buffer <= 16MB
SQLite page cache <= 16-32MB
```

### Executor

2GB 模式下默认 MockExecutor。

约束：

```text
不要同机跑完整 Home Assistant
不要同机跑重型数据库
真实设备 backend 尽量走 HTTP / MQTT / 外部服务
```

预算：

```text
MockExecutor <= 5MB
HTTP / MQTT bridge <= 32-64MB
```

## 推荐验收标准

2GB profile 通过的最低标准：

```text
开机后 MemAvailable >= 1200MB
加载模型后 MemAvailable >= 350MB
连续 50 次单轮请求不 OOM
连续 20 次多轮短时记忆请求不 OOM
dead_loop_rate = 0
schema_valid_rate >= 0.95
LLM timeout 能 fallback
critical memory 下不再发起新 LLM 调用
trace 能落盘并 replay
```

如果加载模型后 `MemAvailable < 250MB`，不要继续调 prompt，先换运行后端或升级内存。

## 实测命令

查看总内存和可用内存：

```bash
free -h
cat /proc/meminfo | egrep 'MemTotal|MemFree|MemAvailable|Buffers|Cached|SwapTotal|SwapFree|CmaTotal|CmaFree'
```

查看进程 RSS：

```bash
ps -eo pid,comm,rss,vsz,%mem --sort=-rss | head -20
```

查看 systemd 服务内存：

```bash
systemctl status ollama
systemctl status edgehome
```

查看 cgroup 内存：

```bash
systemctl show ollama -p MemoryCurrent -p MemoryPeak
systemctl show edgehome -p MemoryCurrent -p MemoryPeak
```

查看 zram：

```bash
zramctl
cat /sys/block/zram0/mm_stat
```

压力测试时记录：

```text
启动前 MemAvailable
加载模型后 MemAvailable
单次请求峰值 RSS
连续请求后 MemAvailable
SwapUsed
OOM / timeout / retry / fallback 次数
```

## 最终判断

2GB 能跑，但必须按极限系统设计：

```text
无桌面
q4 模型
短上下文
短输出
单并发
低内存降级
trace 落盘
长期记忆不常驻
模型后端有硬上限
MemAvailable 保持 300MB 以上
```

对外宣传时建议这样说：

```text
EdgeHome Harness 以 2GB RAM 级边缘设备为设计约束，提供 low_memory profile、上下文预算、输出预算、记忆降级和内存压力策略。2GB 可用于极限验证；实际部署和演示推荐 Raspberry Pi 5 4GB。
```

这比直接说“2GB 可以稳定跑 MiniCPM5-1B + Harness”更准确。

## 参考资料

- Raspberry Pi 5 官方规格：https://www.raspberrypi.com/products/raspberry-pi-5/
- MiniCPM5 Ollama 模型页：https://ollama.com/openbmb/minicpm5
- Linux `/proc/meminfo` 手册：https://man7.org/linux/man-pages/man5/proc_meminfo.5.html
- Linux zram 文档：https://www.kernel.org/doc/html/latest/admin-guide/blockdev/zram.html
