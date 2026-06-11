#!/usr/bin/env bash
set -euo pipefail

QEMU_ROOT="${QEMU_ROOT:-/mnt/e/edgehome-qemu}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${QEMU_ROOT}/cargo-target}"
TARGET="${TARGET:-aarch64-unknown-linux-gnu}"
SSH_KEY="${SSH_KEY:-${QEMU_ROOT}/ssh/edgehome_qemu}"
SSH_PORT="${SSH_PORT:-2222}"
SSH_HOST="${SSH_HOST:-localhost}"
SSH_USER="${SSH_USER:-edge}"
REMOTE_DIR="${REMOTE_DIR:-/home/edge/edgehome}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY="${CARGO_TARGET_DIR}/${TARGET}/release/edgehome"

if [ ! -f "${BINARY}" ]; then
  echo "missing binary: ${BINARY}; run scripts/qemu/build-arm64.sh first." >&2
  exit 1
fi

if [ ! -f "${SSH_KEY}" ]; then
  echo "missing SSH key: ${SSH_KEY}; run scripts/qemu/prepare-image.sh first." >&2
  exit 1
fi

SSH_OPTS=(-p "${SSH_PORT}" -i "${SSH_KEY}" -o StrictHostKeyChecking=accept-new)
SCP_OPTS=(-P "${SSH_PORT}" -i "${SSH_KEY}" -o StrictHostKeyChecking=accept-new)

echo "== Checking SSH connectivity =="
ssh "${SSH_OPTS[@]}" "${SSH_USER}@${SSH_HOST}" "uname -m && mkdir -p '${REMOTE_DIR}/artifacts' '${REMOTE_DIR}/scripts'"

echo "== Copying EdgeHome binary =="
scp "${SCP_OPTS[@]}" "${BINARY}" "${SSH_USER}@${SSH_HOST}:${REMOTE_DIR}/edgehome"

echo "== Copying configs, cases and guest scripts =="
scp "${SCP_OPTS[@]}" -r \
  "${REPO_ROOT}/configs" \
  "${REPO_ROOT}/cases" \
  "${SSH_USER}@${SSH_HOST}:${REMOTE_DIR}/"

scp "${SCP_OPTS[@]}" \
  "${REPO_ROOT}/scripts/qemu/collect-memory.sh" \
  "${REPO_ROOT}/scripts/qemu/run-low-memory-eval.sh" \
  "${SSH_USER}@${SSH_HOST}:${REMOTE_DIR}/scripts/"

ssh "${SSH_OPTS[@]}" "${SSH_USER}@${SSH_HOST}" \
  "chmod +x '${REMOTE_DIR}/edgehome' '${REMOTE_DIR}/scripts/collect-memory.sh' '${REMOTE_DIR}/scripts/run-low-memory-eval.sh' && '${REMOTE_DIR}/edgehome' --help >/dev/null"

echo "Copy complete. SSH into the VM and run:"
echo "  cd ${REMOTE_DIR}"
echo "  bash scripts/collect-memory.sh | tee artifacts/baseline-before-edgehome.txt"
echo "  bash scripts/run-low-memory-eval.sh"
