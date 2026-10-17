# My EdgeHome Agent

This is an independent Cargo application. By default, its SDK dependencies use a
fixed Git commit, `Cargo.lock` records the resolved dependencies, and
`rust-toolchain.toml` selects Rust 1.95.0. Commit all three files with your code.
Another machine needs Rust via rustup, Git, a native linker, and network access
for the first build; it does not need the original SDK checkout.

If you explicitly generated this app with `--sdk-path`, its path dependencies
still need that checkout. Convert them before sharing; see [UPGRADING.md](UPGRADING.md).

## Build and run

```sh
cargo build --locked
cargo run --locked -- doctor --mock
cargo run --locked -- tools
cargo run --locked -- run --mock --prompt "Count the sample document" --tool document.stats
cargo run --locked -- run --mock --prompt "Count the sample document" --tool document.stats --allow document.stats
cargo run --locked -- run --mock --prompt "Read the room sensor" --tool sensor.read --allow sensor.read --events
cargo run --locked -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
```

The two starter tools use immutable in-memory fixtures, not your files or physical
devices. `--mock` is deterministic routing, never an automatic model fallback.
Edit `src/tools.rs` to add your application's tools. Keep validators side-effect
free, check current permissions and state, and implement bounded I/O in handlers.

To use a model, edit `agent.yaml`, run `cargo run --locked -- doctor --probe`, then omit
`--mock`. `--probe` makes two bounded model requests. `--allow TOOL` preauthorizes
that tool for this invocation; review scope before allowing a real handler.
Without `--allow`, `run` only proposes. By default execution returns the tool's
structured receipt; `--loop` enables another model turn after successful results.

`--events` prints metadata lifecycle events to stderr. `--journal session.jsonl`
creates a bounded, metadata-only log at a new path. It cannot restore execution
or resume an old plan. Logs stop accepting writes at 512 KiB.

For a Python/Go host, launch `rpc --mock --enable-execution`. Stdio requests and
responses use one JSON object per line; protocol version 1 is announced at start.
The parent host must approve each plan ID. `client.py` demonstrates the exchange:

```sh
cargo build --locked
python client.py path/to/my-edge-agent
```

RPC is sequential. Its `cancel` command stops the agent when read; an in-flight
blocking call still follows its timeout. Restart the process after cancellation.
The Rust SDK also exposes a cooperative token for another thread to signal.
Pending plans are never persisted or automatically restored. The parent process
owns stdin/stdout and is the trusted authorization boundary; no server port opens.

The fixtures' regression gate measures protocol/host behavior. Run the same cases
with a configured model to measure tool selection and arguments separately. A
valid clarification or exit code 0 alone is not proof of task completion.

See [UPGRADING.md](UPGRADING.md) for SDK upgrades, rollback, and offline builds.
The SDK is a developer preview: fixed versions make builds repeatable, but do not
promise a stable Rust API or qualify your model's task accuracy. Model weights,
inference engine versions, credentials and data must be provisioned separately.
