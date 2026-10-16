# Read-only file agent and clarification loop

This reference app uses real files in an explicitly selected directory. It shows
how an application supplies business preconditions and user input while the
runtime owns task state, call budgets, plan approval and execution receipts.
It is a developer preview, not a qualified unattended agent.

## Run

Prepare a small directory with UTF-8 text files. Set the root explicitly; the app
does not default to the current directory or a user's home. Use an installed
model and a matching configuration. The sample config names a locally prepared
Qwen3 alias described in [model reliability](model-reliability.md).

```powershell
$env:EDGEHOME_FILE_ROOT = 'G:\Data\my-agent-files'
cargo run -p edgehome-kit --example file_agent -- doctor --config configs/file-agent-qwen06.yaml --probe
cargo run -p edgehome-kit --example file_agent -- run --config configs/file-agent-qwen06.yaml --prompt 'Read notes.txt.'
cargo run -p edgehome-kit --example file_agent -- rpc --config configs/file-agent-qwen06.yaml --enable-execution
```

`run` prints a preview unless a tool is explicitly allowed. Use the long-lived
RPC process or Rust SDK for clarification and exact-plan approval. The file app
has no mock model fallback. `EDGEHOME_FILE_POLICY=basic` is an experimental
ablation; the default `guided` enables application-generated clarification.

A [standard-library Python host](../examples/python/file_agent_client.py) displays
questions, accepts user-selected paths and asks for confirmation of each plan:

```sh
cargo build -p edgehome-kit --example file_agent
python examples/python/file_agent_client.py /path/to/file_agent --root /path/to/files --config configs/file-agent-qwen06.yaml
```

On Windows the binary is `file_agent.exe`; with `CARGO_TARGET_DIR` set, find it
under that directory's `debug/examples` folder.

## Resume a task

RPC v1 now advertises `clarification_reply` and `clarification_resolve` in its
ready frame. A clarification result includes its task-local ID, question,
optional tool, retained draft arguments and fields/choices. `status` exposes the
same pending request. Clients send one of these commands, with an actual returned
ID rather than the placeholder below:

```json
{"id":2,"command":{"method":"reply","clarification_id":"returned-id","input":"Use beta/report.txt."}}
{"id":3,"command":{"method":"resolve","clarification_id":"returned-id","values":{"path":"beta/report.txt"}}}
```

`reply` retains the conversation and selected tools, then asks the model again.
Its arguments remain model interpretations, not confirmed facts. `resolve`
requires every requested field, rejects extra fields and invalid choices, and
retains unrelated draft arguments. It skips model inference and repeats type and
business validation. Resolved values are authoritative host input; the host must
obtain them from the user or its own trusted application state. Do not pass a
model's guessed values as user confirmation.

Neither method executes a tool. Display the resulting plan and approve its exact
ID using `execute`. A new prompt or cancellation invalidates pending questions
and plans. IDs are not authentication credentials. Context and model/tool budgets
remain bounded across replies; the per-task clarification limit defaults to 4.
Rejected malformed answers leave a question available for correction. Accepted
answers consume it; policy may return a new question with a new ID.

## Add application policy

Implement `Tool::preflight` or wrap a typed tool in `GuardedTool`. Return
`Preflight::Allow`, `Clarify { message, fields }`, or `Deny`. Fields identify
top-level tool arguments. Candidate values should come from application state,
not invented model output. The wrapper preserves the inner tool's checks.
Callbacks are trusted synchronous code and must bound their own I/O.

Preflight runs when proposing and immediately before dispatch. If policy changes
to clarification after approval, the old plan is consumed, no handler runs, and
`execute` returns `clarification_required`; inspect the pending question. The new
proposal requires new approval. There are no implicit tool retries.

The file app never selects the first matching basename. An unavailable or
incomplete path offers existing paths (at most 32 candidates).
Candidate generation also excludes files this tool cannot currently read, such
as binary or oversized files. Candidates are rechecked after the user selects one.

A missing argument
represented by null produces a structured question. An existing but semantically
wrong model-selected path can still pass: filesystem validation cannot prove
that it matches the user's intent.

## File boundaries

Tools are `file.list`, `file.read` and `file.stats`. They have no write operation.
Paths must be explicit relative paths under the configured root, with forward
slashes. Parent traversal, absolute paths, hidden components, Windows device
names/streams, symlinks and Windows reparse points are rejected or excluded.
The traversal scans at most 128 entries and four nested directory levels; reads
are limited to 16 KiB and must be UTF-8 text without NUL bytes. Output has its own
runtime byte cap (32 KiB in the example configuration). Statistics count bytes,
Unicode scalar values and text lines, not grapheme clusters or tokens.

Use a trusted local directory. Canonicalization and rechecking catch ordinary
changes and static links, but are not an OS sandbox against a hostile process
racing directory replacement, hard links, or blocking filesystem operations.
The synchronous SDK cannot interrupt a blocked kernel call. Stronger isolation
requires platform-specific handle-relative I/O and an OS process boundary.

## Acceptance protocol

See the [actual results and retained failures](file-agent-results.md). The current
model/application profiles do not pass qualification.

`cases/file-agent-acceptance.json` was frozen before implementation and model
outputs (normalized SHA-256
`5d8187746ecc1dc1ca86234f35dadcdced08e66247a0672be8f44eb05711a477`).
It contains 20 tasks: direct use, negation/correction, six clarification tasks,
and six boundary cases. Both arms use the same action protocol and model budget.
Guided mode adds business-generated questions and host field resolution; basic
mode denies unavailable paths and uses free-text follow-ups. This measures that
bundle, not the separate contribution of each feature.

```sh
python scripts/evaluate-file-agent.py --output /path/to/new-evidence-directory
python scripts/verify-file-evidence.py /path/to/new-evidence-directory
```

All example tools are explicitly preauthorized against generated test files.
Every proposed tool executes independently of the answer key, and scoring happens
after receipts are recorded. Scripted user replies are fixed task inputs; no
expected tool/argument/output is sent as feedback. Each of two models runs every
case twice in both modes. Repeated temperature-zero trials are not independent
samples. Model errors, timeouts and malformed output are failures, not successful
refusals. Positive-task completion and boundary handling are reported separately.

Before qualification, qualitatively review every automatically passing
clarification: it must ask for the actual unresolved input, not re-ask already
specified information without explaining the issue, invent available objects,
or promise an unsupported operation. Any unsound question only lowers scores.
This assistant-authored review is not an independent human evaluation. The
experiment stops at one receipt or refusal; it does not validate multi-tool search
or final natural-language summaries. New failures should become regression tests;
future improvements need another unseen acceptance set.

`--model-trace NEWFILE` explicitly captures prompts and responses for reviewable
experiments, up to 2 MiB, without overwrite. It can contain application data.
Default metadata journals remain content-free. Missing evidence fails the model
call; rejected transport bodies are not available to this recorder.
