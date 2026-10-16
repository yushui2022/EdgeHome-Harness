# Clarification and application policy iteration

Goal: complete a bounded clarification → user input → validated proposal →
approved execution loop, with a real read-only filesystem reference application.

Order and modules:
1. Freeze new file-task cases before reading model outputs. Existing model
   reliability cases become regression evidence; do not tune to their scores.
2. `edgehome-runtime`: resumable clarification IDs, structured fields and retained
   draft arguments, host-supplied resolution, free-text replies, application
   preflight outcomes, and dispatch-time revalidation.
3. `edgehome-kit`: RPC exposure and a filesystem example using real files, bounded
   reads and directory enumeration, explicit root, no write tools.
4. Run lifecycle/adversarial tests, real-model paired experiments, independent
   evidence checks, onboarding and release checks. Preserve all failures.

Verification: stale IDs, cancellation, input/context/call limits, no execution
while awaiting input, malformed resolutions, argument retention, changed policy
between proposal and dispatch, path traversal, links, ambiguity, missing files,
oversized/binary content, and real RPC multi-turn use.

Risks: schema and preflight cannot infer arbitrary user intent. Structured host
answers are authoritative; model-generated drafts are not. Synchronous callbacks
cannot be forcibly interrupted. A portable filesystem example is not an OS
sandbox against a hostile process racing filesystem changes. New benchmark cases
are author-written, not an independent blind evaluation. A safer refusal alone
does not count as completion of an actionable task.

Completed 2026-10-16: runtime and RPC clarification, application preflight,
real-file reference tools, interactive Python host, and independent evidence
verification. Both 160-trial runs are retained. The second run is a regression
after filtering unreadable candidates, not a fresh holdout. All 268 Rust tests
and the complete release check passed; the model profiles did not qualify for
unattended execution. See `docs/file-agent-results.md` for counts and limitations.
