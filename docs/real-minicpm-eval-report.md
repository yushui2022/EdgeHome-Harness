# Real MiniCPM / Ollama Eval Report

This report is the place for real model-path evaluation. It must stay separate
from the deterministic mock release gate.

The mock release gate answers:

```text
Did the Rust harness regress?
```

The real MiniCPM/Ollama eval answers:

```text
How does the configured local model behave when it proposes candidate JSON?
```

Do not mix these two claims.

## Status

Workflow added. A current real-model run should be generated with:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run-real-minicpm-eval.ps1
```

If the local Ollama tag differs from the default profile, pass it explicitly:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run-real-minicpm-eval.ps1 -ModelName openbmb/minicpm5:latest
```

The script writes raw JSON output under `artifacts/`, which is intentionally
ignored by git. Summarize stable results in this document only after reviewing
the raw output for private paths, tokens, and environment-specific details.

## Required Environment Metadata

Record these fields for every published real-model report:

| Field | Value |
| --- | --- |
| Date | TODO |
| OS | TODO |
| CPU / device | TODO |
| RAM | TODO |
| Rust version | TODO |
| Ollama version | TODO |
| Model name | TODO |
| Model tag / ID | TODO |
| Profile | TODO |
| Cases file | `cases/zh-home.yaml` |
| Database path | temporary local path, not committed |

## Required Metrics

Record:

| Metric | Value |
| --- | ---: |
| total_cases | TODO |
| category_count | TODO |
| passed | TODO |
| failed | TODO |
| pass_rate | TODO |
| schema_valid_rate | TODO |
| intent_accuracy | TODO |
| slot_accuracy | TODO |
| policy_accuracy | TODO |
| dry_run_accuracy | TODO |
| trace_coverage | TODO |
| false_allow_rate | TODO |
| fail_closed_rate | TODO |
| retry_rate | TODO |
| fallback_rate | TODO |
| dead_loop_rate | TODO |
| latency_avg_ms | TODO |
| latency_p95_ms | TODO |

## Publication Rules

Do not publish a real-model report as a success claim unless:

```text
false_allow_rate = 0.0
fail_closed_rate = 1.0
unsafe / backend-access cases fail closed
raw outputs contain no secrets or private device data
model name and runtime are clearly stated
mock gate metrics are not presented as real-model metrics
```

If real MiniCPM performance is imperfect, report it directly. The point of this
project is that Rust gates unsafe or malformed model output; the model does not
need to be treated as an executor.

## Failure Taxonomy

When a real-model case fails, classify it as one of:

```text
malformed_json
schema_invalid
wrong_intent
wrong_slot
unknown_device
unsupported_capability
policy_mismatch
dry_run_mismatch
retry_exhausted
fallback_used
dead_loop_detected
latency_timeout
```

## Summary Template

Use this section after a real run has been reviewed:

```text
Run date:
Model:
Ollama:
Hardware:
Profile:
Cases:
Pass rate:
Schema valid rate:
False allow rate:
Fail-closed rate:
Latency avg / p95:
Top failure category:
Notes:
```

## Claim Boundary

This report is not a broad natural-language understanding benchmark. It does
not prove production readiness or real-device deployment. It is evidence about
one local model profile inside the EdgeHome Harness command pipeline.
