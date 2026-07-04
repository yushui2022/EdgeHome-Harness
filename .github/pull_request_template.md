## Summary

Describe the change and the layer it touches: parser, registry, gate, adapter,
eval, CLI, docs, or release process.

## Boundary checklist

- [ ] MiniCPM output remains backend-neutral candidate JSON.
- [ ] Backend-specific IDs, routes, topics, tokens, and URLs stay outside the model output.
- [ ] Unsupported devices, capabilities, or backends fail closed.
- [ ] Real device execution remains disabled by default.
- [ ] Public docs do not claim MIoT/Xiaomi, Matter, MQTT, or production gateway support unless the code and tests actually implement it.

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo run -q -p edgehome-cli -- --db-path edgehome-gate.sqlite eval cases/zh-home.yaml --gate`
- [ ] `git diff --check`
