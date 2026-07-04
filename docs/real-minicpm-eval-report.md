# Real MiniCPM / Ollama Eval Report

This report is intentionally separate from the deterministic mock release gate.

The mock release gate answers:

```text
Did the Rust harness regress?
```

The real MiniCPM/Ollama eval answers:

```text
How does the configured local model behave when it proposes candidate JSON?
```

Do not mix these two claims.

## Latest Reviewed Run

Run command:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\run-real-minicpm-eval.ps1 -ModelName openbmb/minicpm5:latest -TimeoutMs 60000 -NumPredict 128
```

Raw artifacts are intentionally kept under `artifacts/`, which is ignored by
git:

```text
artifacts\real-minicpm-eval-openbmb_minicpm5_latest-20260704-212946.json
artifacts\real-minicpm-eval-openbmb_minicpm5_latest-20260704-212946.meta.txt
artifacts\real-minicpm-eval-run-20260704-input-boundary.log
```

## Environment

| Field | Value |
| --- | --- |
| Date | 2026-07-04 21:29:47 +08:00 |
| OS | Microsoft Windows NT 10.0.26200.0 |
| Device | Lenovo 82RC |
| CPU | 12th Gen Intel(R) Core(TM) i5-12500H, 12 cores / 16 logical processors |
| RAM | 16,962,281,472 bytes |
| Rust version | rustc 1.95.0 (59807616e 2026-04-14) |
| Cargo version | cargo 1.95.0 (f2d3ce0bd 2026-03-21) |
| Ollama version | 0.18.3 |
| Model name | openbmb/minicpm5:latest |
| Model ID / size | 08239e8f70e0 / 688 MB |
| Profile | eval_mode |
| Cases file | cases\zh-home.yaml |
| Cases | 108 |
| Timeout / num_predict | 60000 ms / 128 |
| Database path | temporary local path, not committed |

## Metrics

| Metric | Value |
| --- | ---: |
| total_cases | 108 |
| category_count | 12 |
| passed | 104 |
| failed | 4 |
| pass_rate | 0.9630 |
| schema_valid_rate | 0.9500 |
| intent_accuracy | 0.9487 |
| slot_accuracy | 0.9487 |
| policy_accuracy | 0.9800 |
| dry_run_accuracy | 0.9800 |
| trace_coverage | 1.0000 |
| false_allow_rate | 0.0000 |
| fail_closed_rate | 1.0000 |
| retry_rate | 0.0000 |
| fallback_rate | 0.7778 |
| dead_loop_rate | 0.0000 |
| latency_avg_ms | 4829.49 |
| latency_p95_ms | 6023 |
| false_allow_count | 0 |
| fail_closed_count | 39 |
| deterministic_repair_or_fallback_count | 84 |

## Failure Breakdown

Failed cases by trace rejection reason:

| Rejection reason | Failed cases |
| --- | ---: |
| Ollama output governor rejected model response: invalid JSON object missing | 4 |

Failed cases by category:

| Category | Failed cases |
| --- | ---: |
| capability_boundary | 2 |
| normal_control | 1 |
| slot_extraction | 1 |

The compact schema prompt plus deterministic Rust-side candidate repair greatly
improved the end-to-end model+harness path. 84 of 108 cases used deterministic
repair or fallback evidence, so this is not a pure standalone MiniCPM parsing
score. The remaining 4 failures were raw model outputs that the output governor
rejected before normalization because no valid JSON object was available.

## Safety Interpretation

This is a high end-to-end model+harness result, not a standalone model accuracy
result. It should not be presented as proof that MiniCPM alone understands the
full eval set without deterministic Rust repair and gates.

It is useful evidence for the harness boundary:

```text
trace_coverage = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
all 39 expected-blocked cases failed closed
InputBoundaryGate blocked direct backend-access attempts
```

The real model path completed all 108 cases without crashing the eval runner.
When the model returned malformed or governor-rejected output, the harness
produced a traceable deny result with no dry-run plan and no executable plan.
When the model produced incomplete or conflicting slots, deterministic Rust-side
repair could produce a traceable repaired candidate before registry resolution.
That repair is reported through `fallback_rate`; it must stay visible in public
claims.

## Claim Boundary

Allowed:

```text
The real MiniCPM/Ollama path is reproducible, trace-covered, and fail-closed on
the reviewed eval run. The current high pass rate is a model+harness result with
deterministic repair/fallback enabled, not a standalone model parsing benchmark.
```

Not allowed:

```text
Real MiniCPM alone achieves production command accuracy.
Mock gate metrics are real-model metrics.
The model safely outputs vendor-ready JSON.
This run proves broad smart-home language understanding.
Fallback/repair usage can be hidden from the report.
```

## Next Improvements

Improve the model path without weakening the harness boundary:

```text
1. reduce deterministic repair usage while keeping false_allow_rate at 0.0;
2. add stricter negative tests for backend-access wording and vendor payload
   requests;
3. keep fail_closed_rate at 1.0 as a hard gate;
4. continue using the deterministic mock gate as the release regression gate;
5. keep real-device execution separate from model eval.
```
