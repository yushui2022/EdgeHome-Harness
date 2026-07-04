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
artifacts\real-minicpm-eval-openbmb_minicpm5_latest-20260704-203818.json
artifacts\real-minicpm-eval-openbmb_minicpm5_latest-20260704-203818.meta.txt
artifacts\real-minicpm-eval-run-20260704-traceable.log
```

## Environment

| Field | Value |
| --- | --- |
| Date | 2026-07-04 20:38:18 +08:00 |
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
| passed | 30 |
| failed | 78 |
| pass_rate | 0.2778 |
| schema_valid_rate | 0.8800 |
| intent_accuracy | 0.0256 |
| slot_accuracy | 0.0000 |
| policy_accuracy | 0.3900 |
| dry_run_accuracy | 0.3900 |
| trace_coverage | 1.0000 |
| false_allow_rate | 0.0000 |
| fail_closed_rate | 1.0000 |
| retry_rate | 0.0000 |
| fallback_rate | 0.0000 |
| dead_loop_rate | 0.0000 |
| latency_avg_ms | 2638.64 |
| latency_p95_ms | 4054 |
| false_allow_count | 0 |
| fail_closed_count | 39 |

## Failure Breakdown

Failed cases by trace rejection reason:

| Rejection reason | Failed cases |
| --- | ---: |
| SchemaGate: intent is unknown | 67 |
| Ollama output governor rejected model response | 7 |
| SchemaGate: room is unknown | 4 |

Failed cases by category:

| Category | Failed cases |
| --- | ---: |
| air_conditioner_controls | 12 |
| normal_control | 12 |
| slot_extraction | 12 |
| high_risk_policy | 10 |
| capability_boundary | 10 |
| runtime_memory | 8 |
| long_memory | 7 |
| fail_closed_safety | 7 |

The model frequently produced schema-valid JSON whose semantic slots stayed
`unknown`, especially `intent`, so the Rust `SchemaGate` rejected the command
before dry-run planning. In 7 failed cases the output governor rejected the raw
model response before normalization.

## Safety Interpretation

This is not a high natural-language accuracy result. It should not be presented
as proof that the current MiniCPM profile understands the full eval set.

It is useful evidence for the harness boundary:

```text
trace_coverage = 1.0
false_allow_rate = 0.0
fail_closed_rate = 1.0
all 39 expected-blocked cases failed closed
```

The real model path completed all 108 cases without crashing the eval runner.
When the model returned unknown, malformed, or governor-rejected output, the
harness produced a traceable deny result with no dry-run plan and no executable
plan.

## Claim Boundary

Allowed:

```text
The real MiniCPM/Ollama path is reproducible, trace-covered, and fail-closed on
the reviewed eval run. The current profile still has low command accuracy and
needs prompt/model tuning before being presented as a strong parser.
```

Not allowed:

```text
Real MiniCPM achieves production command accuracy.
Mock gate metrics are real-model metrics.
The model safely outputs vendor-ready JSON.
This run proves broad smart-home language understanding.
```

## Next Improvements

Improve the model path without weakening the harness boundary:

```text
1. tighten the MiniCPM system prompt around the canonical JSON enum values;
2. add a small repair/retry loop only inside the output-governor boundary;
3. keep false_allow_rate at 0.0 and fail_closed_rate at 1.0 as hard gates;
4. continue using the deterministic mock gate as the release regression gate;
5. keep real-device execution separate from model eval.
```
