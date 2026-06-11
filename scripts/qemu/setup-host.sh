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
  jq

echo "== Tool versions =="
qemu-system-aarch64 --version
qemu-img --version
if command -v cloud-localds >/dev/null 2>&1; then
  cloud-localds --version || true
else
  echo "cloud-localds not found; check cloud-image-utils installation." >&2
  exit 1
fi

echo "Host setup complete."
