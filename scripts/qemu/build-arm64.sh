#!/usr/bin/env bash
set -euo pipefail

QEMU_ROOT="${QEMU_ROOT:-/mnt/e/edgehome-qemu}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${QEMU_ROOT}/cargo-target}"
TARGET="${TARGET:-aarch64-unknown-linux-gnu}"
LINKER="${LINKER:-aarch64-linux-gnu-gcc}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}"

echo "== EdgeHome ARM64 build =="
echo "REPO_ROOT=${REPO_ROOT}"
echo "CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
echo "TARGET=${TARGET}"

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup not found. Install Rust in WSL2 before running this script." >&2
  exit 1
fi

if ! command -v "${LINKER}" >/dev/null 2>&1; then
  echo "${LINKER} not found. Install gcc-aarch64-linux-gnu first." >&2
  echo "sudo apt install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu pkg-config" >&2
  exit 1
fi

rustup target add "${TARGET}"
mkdir -p "${CARGO_TARGET_DIR}"

export CARGO_TARGET_DIR
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${LINKER}"

echo "== cargo check --workspace =="
cargo check --workspace

echo "== cargo build --release --target ${TARGET} -p edgehome-cli =="
cargo build --release --target "${TARGET}" -p edgehome-cli

BINARY="${CARGO_TARGET_DIR}/${TARGET}/release/edgehome"
if [ ! -f "${BINARY}" ]; then
  echo "expected binary not found: ${BINARY}" >&2
  exit 1
fi

ls -lh "${BINARY}"
file "${BINARY}" || true

echo "ARM64 build complete: ${BINARY}"

