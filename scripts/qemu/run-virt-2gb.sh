#!/usr/bin/env bash
set -euo pipefail

QEMU_ROOT="${QEMU_ROOT:-/mnt/e/edgehome-qemu}"
IMAGE_DIR="${QEMU_ROOT}/images"
IMAGE_FILE="${IMAGE_FILE:-ubuntu-24.04-arm64.qcow2}"
SSH_PORT="${SSH_PORT:-2222}"
RAM_MB="${RAM_MB:-2048}"
SMP="${SMP:-2}"

cd "${IMAGE_DIR}"

for file in "${IMAGE_FILE}" seed.img AAVMF_CODE.fd AAVMF_VARS.fd; do
  if [ ! -f "${file}" ]; then
    echo "missing ${IMAGE_DIR}/${file}; run scripts/qemu/prepare-image.sh first." >&2
    exit 1
  fi
done

if [ "${RAM_MB}" != "2048" ]; then
  echo "warning: RAM_MB=${RAM_MB}; embedded validation plan expects 2048."
fi

echo "== Starting QEMU virt ARM64 =="
echo "RAM_MB=${RAM_MB}"
echo "SSH_PORT=${SSH_PORT}"
echo "Use 'ssh edge@localhost -p ${SSH_PORT}' after cloud-init completes."
echo "QEMU console exit: Ctrl+A then X, or run 'sudo poweroff' inside the VM."

exec qemu-system-aarch64 \
  -machine virt \
  -cpu cortex-a72 \
  -smp "${SMP}" \
  -m "${RAM_MB}" \
  -nographic \
  -drive if=pflash,format=raw,readonly=on,file=AAVMF_CODE.fd \
  -drive if=pflash,format=raw,file=AAVMF_VARS.fd \
  -drive if=virtio,format=qcow2,file="${IMAGE_FILE}" \
  -drive if=virtio,format=raw,file=seed.img \
  -netdev user,id=net0,hostfwd=tcp::"${SSH_PORT}"-:22 \
  -device virtio-net-pci,netdev=net0

