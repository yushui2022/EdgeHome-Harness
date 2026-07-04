# Security Policy

EdgeHome Harness is a prototype safety harness for smart-home command pipelines.
Please treat security issues carefully because the project is explicitly about
command validation, policy gates, backend boundaries, and real-device execution
being disabled by default.

## Supported Scope

Security reports are accepted for the current `main` branch and the latest
documented `0.1.x` prototype baseline.

Important current limitations:

- Real device execution is disabled by default.
- Mock and Home Assistant demo dry-run paths are implemented.
- MIoT/Xiaomi, Matter, and MQTT are future adapter targets, not implemented
  backend support today.

## What To Report

Please report:

- A path where malformed or unsafe model output becomes an allowed command.
- A path where unsupported devices, actions, or backends do not fail closed.
- Leakage of tokens, backend URLs, entity IDs, or secrets into traces, logs,
  dry-run payloads, or eval reports.
- Prompt-injection or backend-access inputs that bypass the input guard.
- Real execution becoming enabled without explicit operator configuration.
- Adapter behavior that silently falls back to mock payloads.

## What Not To Include Publicly

Do not post real secrets or exploitable details in a public issue:

- Home Assistant tokens.
- MIoT or miIO tokens.
- Private URLs or LAN topology.
- Real device IDs that should remain private.
- Step-by-step exploit details for bypassing a safety gate.

## Reporting

Use GitHub private vulnerability reporting when available:

https://github.com/yushui2022/EdgeHome-Harness/security/advisories/new

If private reporting is not available, open a minimal public issue that says a
security report is available, but do not include secrets or exploit details.

## Response Expectations

This is an early prototype, so response times may vary. Maintainers should aim
to:

- Acknowledge valid reports.
- Reproduce the issue with a minimal case.
- Add regression coverage before publishing the fix.
- Update docs if the issue changes public scope or safety claims.
