# My EdgeHome Agent

This is an independent Cargo application using local EdgeHome SDK dependencies.
Keep the SDK checkout available at the paths recorded in `Cargo.toml`.

```sh
cargo run -- doctor --mock
cargo run -- tools
cargo run -- run --mock --prompt "Count the sample document" --tool document.stats
cargo run -- run --mock --prompt "Count the sample document" --tool document.stats --allow document.stats
cargo run -- run --mock --prompt "Read the room sensor" --tool sensor.read --allow sensor.read --events
cargo run -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
```

The two starter tools use immutable in-memory fixtures, not your files or physical
devices. `--mock` is deterministic routing, never an automatic model fallback.
Edit `src/tools.rs` to add your application's tools. Keep validators side-effect
free, check current permissions and state, and implement bounded I/O in handlers.

To use a model, edit `agent.yaml`, run `cargo run -- doctor --probe`, then omit
`--mock`. `--probe` makes one bounded model request. `--allow TOOL` preauthorizes
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
cargo build
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
