#!/usr/bin/env bash
set -euo pipefail

QEMU_ROOT="${QEMU_ROOT:-/mnt/e/edgehome-qemu}"
IMAGE_DIR="${QEMU_ROOT}/images"
SSH_DIR="${QEMU_ROOT}/ssh"
SSH_KEY="${SSH_KEY:-${SSH_DIR}/edgehome_qemu}"
IMAGE_URL="${IMAGE_URL:-https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-arm64.img}"
IMAGE_FILE="${IMAGE_FILE:-ubuntu-24.04-arm64.qcow2}"
IMAGE_SIZE="${IMAGE_SIZE:-20G}"

mkdir -p "${IMAGE_DIR}"
mkdir -p "${SSH_DIR}"
cd "${IMAGE_DIR}"

echo "== Preparing Ubuntu ARM64 cloud image =="
echo "IMAGE_DIR=${IMAGE_DIR}"
echo "IMAGE_URL=${IMAGE_URL}"

if [ ! -f "${IMAGE_FILE}" ]; then
  wget "${IMAGE_URL}" -O "${IMAGE_FILE}"
else
  echo "Image already exists: ${IMAGE_FILE}"
fi

echo "== Resizing image to ${IMAGE_SIZE} =="
qemu-img resize "${IMAGE_FILE}" "${IMAGE_SIZE}"
qemu-img info "${IMAGE_FILE}"

if [ ! -f "${SSH_KEY}" ]; then
  echo "== Generating SSH key for QEMU guest automation =="
  ssh-keygen -t ed25519 -N "" -f "${SSH_KEY}" -C "edgehome-qemu" >/dev/null
fi

PUB_KEY="$(cat "${SSH_KEY}.pub")"

echo "== Writing cloud-init user-data =="
cat > user-data <<EOF
#cloud-config
hostname: edgehome-qemu
manage_etc_hosts: true

users:
  - name: edge
    groups: sudo
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: false
    plain_text_passwd: edgehome
    ssh_authorized_keys:
      - ${PUB_KEY}

ssh_pwauth: true
disable_root: false

package_update: true
packages:
  - curl
  - git
  - build-essential
  - pkg-config
  - sqlite3
  - htop
  - jq
  - ca-certificates

runcmd:
  - [ sh, -c, "echo 'edge ALL=(ALL) NOPASSWD:ALL' >/etc/sudoers.d/edge" ]
EOF

cat > meta-data <<'EOF'
instance-id: edgehome-qemu
local-hostname: edgehome-qemu
EOF

echo "== Creating cloud-init seed.img =="
cloud-localds seed.img user-data meta-data

echo "== Copying AAVMF firmware =="
AAVMF_CODE_SRC="${AAVMF_CODE_SRC:-}"
AAVMF_VARS_SRC="${AAVMF_VARS_SRC:-}"

if [ -z "${AAVMF_CODE_SRC}" ]; then
  AAVMF_CODE_SRC="$(find /usr/share -name 'AAVMF_CODE.fd' 2>/dev/null | head -n 1 || true)"
fi

if [ -z "${AAVMF_VARS_SRC}" ]; then
  AAVMF_VARS_SRC="$(find /usr/share -name 'AAVMF_VARS.fd' 2>/dev/null | head -n 1 || true)"
fi

if [ -z "${AAVMF_CODE_SRC}" ] || [ -z "${AAVMF_VARS_SRC}" ]; then
  echo "AAVMF firmware not found. Install qemu-efi-aarch64 or set AAVMF_CODE_SRC/AAVMF_VARS_SRC." >&2
  exit 1
fi

cp "${AAVMF_CODE_SRC}" ./AAVMF_CODE.fd
cp "${AAVMF_VARS_SRC}" ./AAVMF_VARS.fd

echo "== Prepared files =="
ls -lh "${IMAGE_FILE}" seed.img AAVMF_CODE.fd AAVMF_VARS.fd

echo "Image preparation complete."
