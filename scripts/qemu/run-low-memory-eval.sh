#!/usr/bin/env bash
set -euo pipefail

EDGEHOME_BIN="${EDGEHOME_BIN:-./edgehome}"
CONFIG_DIR="${CONFIG_DIR:-configs}"
CASES_PATH="${CASES_PATH:-cases/zh-home.yaml}"
ARTIFACT_DIR="${ARTIFACT_DIR:-artifacts}"

mkdir -p "${ARTIFACT_DIR}"

if [ ! -x "${EDGEHOME_BIN}" ]; then
  echo "edgehome binary not executable: ${EDGEHOME_BIN}" >&2
  exit 1
fi

echo "== Runtime baseline =="
"$(dirname "${BASH_SOURCE[0]}")/collect-memory.sh" | tee "${ARTIFACT_DIR}/baseline-before-edgehome.txt"

echo "== Config show: low_memory =="
"${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" config show \
  | tee "${ARTIFACT_DIR}/config-show-low-memory.json"

jq '.name, .model_name, .num_ctx, .num_predict, .retry_count, .memory_enabled, .executor_backend' \
  "${ARTIFACT_DIR}/config-show-low-memory.json"

"$(dirname "${BASH_SOURCE[0]}")/collect-memory.sh" | tee "${ARTIFACT_DIR}/after-edgehome-config-show.txt"

echo "== Mock parse =="
"${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" --db-path "${ARTIFACT_DIR}/qemu-parse.sqlite" \
  parse --mock "把卧室空调打开" \
  | tee "${ARTIFACT_DIR}/mock-parse-open-bedroom-ac.json"

jq '.' "${ARTIFACT_DIR}/mock-parse-open-bedroom-ac.json" >/dev/null

echo "== Mock dry-run =="
"${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" --db-path "${ARTIFACT_DIR}/qemu-dry-run.sqlite" \
  dry-run --mock "把卧室空调打开" \
  | tee "${ARTIFACT_DIR}/mock-dry-run-open-bedroom-ac.json"

jq '.trace_id, .policy_decision, .dry_run_plan' "${ARTIFACT_DIR}/mock-dry-run-open-bedroom-ac.json"

echo "== Eval low_memory =="
"${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" --db-path "${ARTIFACT_DIR}/qemu-eval.sqlite" \
  eval "${CASES_PATH}" \
  | tee "${ARTIFACT_DIR}/eval-low-memory.json"

jq '.report' "${ARTIFACT_DIR}/eval-low-memory.json"

echo "== Eval low_memory release gate =="
"${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" --db-path "${ARTIFACT_DIR}/qemu-gate.sqlite" \
  eval "${CASES_PATH}" --gate \
  | tee "${ARTIFACT_DIR}/eval-low-memory-gate.json"

jq '.gate.passed, .gate.checks' "${ARTIFACT_DIR}/eval-low-memory-gate.json"

"$(dirname "${BASH_SOURCE[0]}")/collect-memory.sh" | tee "${ARTIFACT_DIR}/after-eval-gate.txt"

echo "== Resource pressure policy =="
for free_memory_mb in 1024 400 128; do
  "${EDGEHOME_BIN}" --profile low_memory --config-dir "${CONFIG_DIR}" \
    config pressure --free-memory-mb "${free_memory_mb}" \
    | tee "${ARTIFACT_DIR}/pressure-${free_memory_mb}.json"
  jq '.' "${ARTIFACT_DIR}/pressure-${free_memory_mb}.json" >/dev/null
done

"$(dirname "${BASH_SOURCE[0]}")/collect-memory.sh" | tee "${ARTIFACT_DIR}/after-pressure-policy.txt"

echo "== Low-memory validation summary =="
jq '{
  profile,
  report,
  gate
}' "${ARTIFACT_DIR}/eval-low-memory-gate.json" \
  | tee "${ARTIFACT_DIR}/low-memory-validation-summary.json"

echo "Low-memory mock harness validation complete."

