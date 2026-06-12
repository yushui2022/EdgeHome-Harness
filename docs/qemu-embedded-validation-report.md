# QEMU Embedded Validation Report

Date: 2026-06-12

This report records the current embedded validation progress for EdgeHome Harness.

The purpose of this validation track is to prove that EdgeHome Harness can be exercised in an ARM64 Linux environment with a strict 2GB RAM limit before moving to a real Raspberry Pi or similar board. This is a pre-validation environment, not a real hardware performance benchmark.

## Scope

Validated target:

```text
WSL2 host on Windows
QEMU virt ARM64 guest
Ubuntu 24.04 ARM64 cloud image
Guest RAM: 2048MB
Disk and artifacts under E:\edgehome-qemu
```

Out of scope for this report:

```text
Real Raspberry Pi token/s
Real thermal behavior
Real Mijia / MIoT / Matter device control
Ollama / MiniCPM5 long-running benchmark
Commercial smart-home gateway readiness
```

## Completed Evidence

The QEMU ARM64 guest booted successfully with the expected architecture and memory budget.

Captured guest baseline:

```text
Linux edgehome-qemu 6.8.0-117-generic #117-Ubuntu SMP PREEMPT_DYNAMIC Thu May  7 17:26:37 UTC 2026 aarch64 aarch64 aarch64 GNU/Linux

               total        used        free      shared  buff/cache   available
Mem:           1.9Gi       227Mi       792Mi       1.1Mi       1.0Gi       1.7Gi
Swap:             0B          0B          0B

MemTotal:        2002200 kB
MemAvailable:    1769284 kB
SwapTotal:             0 kB
SwapFree:              0 kB

cloud-init status: done
```

This proves:

```text
QEMU virt ARM64 guest can boot.
Guest architecture is aarch64.
Guest memory is locked to the 2GB class.
Cloud-init completed.
SSH access was proven after copying the generated private key from /mnt/e to ~/.ssh with chmod 600.
```

## Host Preparation Completed

Completed:

```text
E:\edgehome-qemu directory layout created.
Ubuntu 24.04 ARM64 qcow2 downloaded to E:\edgehome-qemu\images.
QEMU image resized to 20G virtual size.
cloud-init seed.img generated.
AAVMF_CODE.fd and AAVMF_VARS.fd copied.
QEMU host scripts added under scripts/qemu.
```

Script commits:

```text
e401d3a chore: add qemu validation scripts
e959b6c chore: automate qemu guest ssh access
```

## Current Blocker

The current blocker is not QEMU.

The QEMU ARM64 guest is already validated. The incomplete part is the host-side Rust toolchain required for cross-compiling `edgehome` to `aarch64-unknown-linux-gnu`.

Observed problem:

```text
rustup downloaded/created a partial stable-x86_64-unknown-linux-gnu toolchain,
but the toolchain manifest was missing.

rustup default stable failed with:
Missing manifest in toolchain 'stable-x86_64-unknown-linux-gnu'
```

Ubuntu apt is not a valid fallback because Ubuntu 24.04 currently provides Rust 1.75, while the project declares:

```text
rust-version = 1.95
```

Therefore, the next step must repair the rustup toolchain rather than silently using apt cargo/rustc.

Recommended repair inside WSL:

```bash
rm -rf ~/.rustup/toolchains/stable-* ~/.rustup/downloads/* ~/.rustup/tmp/*
export RUSTUP_CONCURRENT_DOWNLOADS=1
export RUSTUP_DOWNLOAD_TIMEOUT=300
rustup toolchain install stable --profile minimal --target aarch64-unknown-linux-gnu --no-self-update
rustup default stable
rustc --version
cargo --version
```

## WSL State Note

During the interrupted run, the imported WSL distribution name became unavailable to `wsl -d Ubuntu-24.04-EdgeHome`, while its install directory remained on E drive:

```text
E:\wsl\Ubuntu-24.04
```

Before continuing the build phase, re-check:

```powershell
wsl -l -v
```

If the distro is missing, re-import or install Ubuntu WSL2 on E drive, then rerun:

```bash
bash scripts/qemu/setup-host.sh
```

## Remaining Validation Steps

Next required steps:

```text
1. Restore WSL host availability.
2. Repair Rust stable toolchain using rustup, not apt rustc.
3. Build edgehome for aarch64-unknown-linux-gnu.
4. Copy edgehome binary, configs, cases and guest scripts into the QEMU guest.
5. Run config show, parse --mock, dry-run --mock, eval, eval --gate.
6. Run config pressure at 1024 / 400 / 128 MB.
7. Append eval and pressure results to this report.
```

Completion criteria still open:

```text
ARM64 edgehome binary runs inside guest.
low_memory eval gate passes.
pressure policy outputs are recorded.
Trace/eval artifacts are collected.
```

## Interpretation

This report currently proves the embedded environment boot layer:

```text
ARM64 Linux + 2GB RAM + cloud-init + SSH access
```

It does not yet prove the full EdgeHome Harness embedded validation chain:

```text
ARM64 Rust binary + low_memory eval gate + pressure policy
```

The next work should continue from Rust toolchain repair and ARM64 build, not restart the QEMU design.
