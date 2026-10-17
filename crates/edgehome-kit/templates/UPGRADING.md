# Upgrade and move this application

## Rebuild on another machine

Commit `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, source, cases and
configuration without secrets. Install rustup, Git and your platform's native
linker (Linux C compiler; Windows MSVC Build Tools or a working GNU toolchain;
macOS Command Line Tools). Clone the **application**, then run:

```sh
cargo build --locked
cargo run --locked -- doctor --mock
cargo run --locked -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
```

Rustup selects the pinned compiler. `--locked` rejects dependency drift instead
of silently changing the lockfile. A fresh machine needs network access to Rust,
the SDK Git repository and crates.io. The original SDK checkout is not required
for Git dependencies. For offline builds, provision the compiler/linker and
populate Cargo's cache with `cargo fetch --locked` on that machine first, then
use `cargo build --frozen`. Moving only the application folder does not move
those caches. A compiled executable is specific to its OS, architecture and ABI.

## Upgrade the SDK deliberately

1. Commit a working application baseline. Read the target revision's changelog
   and compatibility notes. The SDK is a developer preview; Rust APIs and
   configuration may change between revisions, including within `0.1.0`.
2. In `Cargo.toml`, replace **both** SDK `rev` values with the same published,
   full 40-character commit from the official repository. Update any directly
   used `edgehome-inference` dependency to that same revision. Do not use `main`
   as a version. Updating the global `edgehome-dev` CLI alone does not change
   this application's SDK or copy new templates over your tool code.
3. Reconcile the lockfile conservatively, inspect its diff, then validate:

```sh
cargo update --workspace
cargo build --locked
cargo test --locked
cargo run --locked -- doctor --mock
cargo run --locked -- eval --mock --cases cases.json --allow document.stats --allow sensor.read
```

Use your application's cases and authorized tools once you replace the starter
fixtures. Review changes to `rust-version` and the target SDK's
`rust-toolchain.toml`; update this application's toolchain pin explicitly if
required. Run `doctor --probe` and your real-model regression cases before
deploying. Mock success does not establish model reliability.

4. Commit the manifest, lockfile, toolchain and any API/config migration together.
   Keep the previous application commit and executable until rollout succeeds.
   To roll back, check out that complete application commit and build with
   `--locked`. Restoring only the manifest can leave incompatible source/config.
   External data changes made by your tools need an application-specific rollback.

## Convert an older path-based application

Replace SDK `path` entries with `git` and `rev` (substitute a real commit):

```toml
[dependencies]
edgehome-kit = { git = "https://github.com/yushui2022/EdgeHome-Harness.git", rev = "<FULL_40_CHARACTER_COMMIT>" }
edgehome-runtime = { git = "https://github.com/yushui2022/EdgeHome-Harness.git", rev = "<FULL_40_CHARACTER_COMMIT>" }
```

Keep your other dependencies. Add the chosen SDK's `rust-toolchain.toml`, retain
your existing `Cargo.lock`, then follow the upgrade validation above. Do not run
`init` over an existing app; it intentionally refuses to overwrite your files.

Model engine, model digest/quantization, chat template, endpoints and external
tool services are separate deployment inputs; Cargo does not install or pin
them. Record them with the application's model evaluation evidence.
