#!/usr/bin/env bash
set -euo pipefail

QEMU_ROOT="${QEMU_ROOT:-/mnt/e/edgehome-qemu}"

echo "== EdgeHome QEMU host setup =="
echo "QEMU_ROOT=${QEMU_ROOT}"

run_as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  else
    sudo "$@"
  fi
}

if ! grep -qi microsoft /proc/version 2>/dev/null; then
  echo "warning: this script is intended for WSL2 Ubuntu, but /proc/version does not look like WSL."
fi

mkdir -p "${QEMU_ROOT}/images"
mkdir -p "${QEMU_ROOT}/cargo-target"
mkdir -p "${QEMU_ROOT}/artifacts"
mkdir -p "${QEMU_ROOT}/scripts"
mkdir -p "${QEMU_ROOT}/repo-copy"

echo "== Disk space for QEMU root =="
df -h "${QEMU_ROOT}"

echo "== Installing QEMU and helper packages =="
run_as_root apt update
run_as_root apt install -y \
  qemu-system-arm \
  qemu-utils \
  qemu-efi-aarch64 \
  cloud-image-utils \
  wget \
  curl \
  xz-utils \
  openssh-client \
  ca-certificates \
  git \
  jq \
  build-essential \
  pkg-config \
  gcc-aarch64-linux-gnu \
  g++-aarch64-linux-gnu

echo "== Tool versions =="
qemu-system-aarch64 --version
qemu-img --version
if command -v cloud-localds >/dev/null 2>&1; then
  echo "cloud-localds found: $(command -v cloud-localds)"
else
  echo "cloud-localds not found; check cloud-image-utils installation." >&2
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "== Installing Rust toolchain with rustup =="
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi

if [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "${HOME}/.cargo/env"
fi

if ! timeout 20 rustup toolchain list >/dev/null 2>&1; then
  echo "rustup is present but not healthy; remove the broken toolchain/cache and rerun this script." >&2
  echo "Suggested manual cleanup inside WSL:" >&2
  echo "  rm -rf ~/.rustup/toolchains/stable-* ~/.rustup/downloads/* ~/.rustup/tmp/*" >&2
  exit 1
fi

if ! rustc --version >/dev/null 2>&1 || ! cargo --version >/dev/null 2>&1; then
  export RUSTUP_CONCURRENT_DOWNLOADS="${RUSTUP_CONCURRENT_DOWNLOADS:-1}"
  export RUSTUP_DOWNLOAD_TIMEOUT="${RUSTUP_DOWNLOAD_TIMEOUT:-300}"
  rustup toolchain install stable --profile minimal --target aarch64-unknown-linux-gnu --no-self-update
  rustup default stable
else
  rustup target add aarch64-unknown-linux-gnu
fi

aarch64-linux-gnu-gcc --version | head -n 1
cargo --version
rustc --version

echo "Host setup complete."
