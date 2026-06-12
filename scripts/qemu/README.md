# QEMU Embedded Validation Scripts

这些脚本服务于根目录的 `嵌入式验证计划.md`。

目标不是把 QEMU 伪装成真实树莓派，而是建立一个可复现的 ARM64 Linux + 2GB RAM 预验证环境，用来验证 EdgeHome Harness 的 low_memory profile、ARM64 二进制、mock eval gate、内存压力策略和 trace/report 链路。

## 执行顺序

在 WSL2 Ubuntu 宿主环境执行：

```bash
cd /path/to/EdgeHome-Harness

bash scripts/qemu/setup-host.sh
bash scripts/qemu/prepare-image.sh
bash scripts/qemu/run-virt-2gb.sh
```

QEMU 启动后，另开一个 WSL2 终端执行：

```bash
mkdir -p ~/.ssh
cp /mnt/e/edgehome-qemu/ssh/edgehome_qemu ~/.ssh/edgehome_qemu
chmod 600 ~/.ssh/edgehome_qemu
ssh -i ~/.ssh/edgehome_qemu edge@localhost -p 2222
```

确认 VM 可登录后，在 WSL2 宿主环境交叉编译并复制产物：

```bash
bash scripts/qemu/build-arm64.sh
bash scripts/qemu/copy-edgehome.sh
```

然后进入 VM 执行：

```bash
cd /home/edge/edgehome
bash scripts/collect-memory.sh | tee artifacts/baseline-before-edgehome.txt
bash scripts/run-low-memory-eval.sh
```

## 默认目录

脚本默认使用：

```text
/mnt/e/edgehome-qemu
```

对应 Windows 路径：

```text
E:\edgehome-qemu
```

`prepare-image.sh` 会自动生成：

```text
/mnt/e/edgehome-qemu/ssh/edgehome_qemu
/mnt/e/edgehome-qemu/ssh/edgehome_qemu.pub
```

公钥会写入 cloud-init，后续 `copy-edgehome.sh` 可以免密码复制二进制和配置。

注意：私钥放在 `/mnt/e` 时，WSL 会看到 0777 权限，OpenSSH 客户端会拒绝使用。`copy-edgehome.sh` 会自动把私钥复制到 `~/.ssh/edgehome_qemu` 并设置 `chmod 600`。

目录用途：

```text
images/        QEMU 镜像、cloud-init seed、UEFI vars
cargo-target/  Rust 交叉编译 target
artifacts/     宿主机侧报告素材
scripts/       可选宿主机辅助脚本
repo-copy/     可选 ASCII 路径仓库副本
```

## 关键约束

`run-virt-2gb.sh` 固定使用：

```bash
-m 2048
```

这才是 2GB RAM 约束。E 盘只解决磁盘空间，不增加 VM 内存。

## 验证主线

必须先跑通 mock harness：

```text
config show
parse --mock
dry-run --mock
eval cases/zh-home.yaml
eval cases/zh-home.yaml --gate
config pressure 1024 / 400 / 128
```

Ollama / MiniCPM5 只作为后续可选边界验证，不阻塞 Rust Harness 主线。

## 不提交的文件

不要提交：

```text
*.qcow2
seed.img
AAVMF_CODE.fd
AAVMF_VARS.fd
cargo-target/
*.sqlite
模型权重
```
