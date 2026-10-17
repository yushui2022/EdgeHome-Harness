"""Exercise installation, portable Git dependencies, upgrades and the RPC host.

Python 3.11+. Git transport uses a temporary local remote containing this working
tree, so PRs test their own code without depending on a published release tag.
No global Git configuration is changed. Public-network availability is separate.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
os.environ["CARGO_TARGET_DIR"] = str(TARGET)
SUFFIX = ".exe" if os.name == "nt" else ""
REPOSITORY = "https://github.com/yushui2022/EdgeHome-Harness.git"
BASELINE = (ROOT / "crates/edgehome-kit/sdk-revision.txt").read_text(encoding="utf-8").strip()


def run(arguments, cwd, *, check=True, env=None):
    result = subprocess.run(
        [str(arg) for arg in arguments], cwd=cwd, text=True,
        encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=600, check=False, env=env,
    )
    if check and result.returncode:
        raise RuntimeError(f"command failed: {arguments}\n{result.stdout}\n{result.stderr}")
    return result


def exercise(project, name):
    before = (project / "Cargo.lock").read_bytes()
    run(["cargo", "build", "--locked", "--offline"], project)
    assert (project / "Cargo.lock").read_bytes() == before
    # Run the generated application, not an SDK example binary.
    application = TARGET / "debug" / (name + SUFFIX)
    ready = json.loads(run([application, "doctor", "--mock"], project).stdout)
    assert ready["status"] == "ready"
    assert set(ready["tools"]) == {"document.stats", "sensor.read"}

    for tool, prompt, expected in [
        ("document.stats", "Count the sample document", {"characters": 84, "lines": 2}),
        ("sensor.read", "Read the room sensor", {"temperature_c": 22.5, "source": "fixture"}),
    ]:
        preview = run([application, "run", "--mock", "--prompt", prompt, "--tool", tool], project)
        lines = [json.loads(line) for line in preview.stdout.splitlines()]
        assert len(lines) == 1 and lines[0]["step"]["status"] == "proposal"
        completed = run([application, "run", "--mock", "--prompt", prompt, "--tool", tool, "--allow", tool, "--events"], project)
        lines = [json.loads(line) for line in completed.stdout.splitlines()]
        assert lines[-1]["receipt"]["output"] == expected
        events = [json.loads(line) for line in completed.stderr.splitlines()]
        assert any(event["event"]["kind"] == "model_started" for event in events)

    run([sys.executable, project / "client.py", application], project)
    evaluated = json.loads(run([application, "eval", "--mock", "--cases", "cases.json", "--allow", "document.stats", "--allow", "sensor.read"], project).stdout)
    assert evaluated["passed"] == evaluated["total"] == 8


def commit_fixture(repository):
    run(["git", "add", "."], repository)
    run(["git", "-c", "user.name=SDK onboarding", "-c", "user.email=sdk-test@example.invalid",
         "-c", "commit.gpgsign=false", "commit", "-m", "SDK installation fixture"], repository)
    return run(["git", "rev-parse", "HEAD"], repository).stdout.strip()


def verify_lock(project, revision):
    packages = tomllib.loads((project / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    sdk = [package for package in packages if package["name"].startswith("edgehome-")]
    assert {package["name"] for package in sdk} == {"edgehome-kit", "edgehome-runtime", "edgehome-inference"}
    assert all(package["source"] == f"git+{REPOSITORY}?rev={revision}#{revision}" for package in sdk)
    baseline = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    signature = lambda p: (p["name"], p["version"], p.get("source"), p.get("checksum"))
    pinned = {signature(package) for package in baseline}
    assert all(signature(p) in pinned for p in packages if p.get("source", "").startswith("registry+"))
    assert tomllib.loads((project / "rust-toolchain.toml").read_text(encoding="utf-8")) == tomllib.loads(
        (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
    )


def main():
    run(["cargo", "build", "--locked", "-p", "edgehome-kit", "--bin", "edgehome-dev"], ROOT)
    with tempfile.TemporaryDirectory(prefix="edgehome-onboarding-") as directory:
        owned = Path(directory).resolve()
        assert owned.parent == Path(tempfile.gettempdir()).resolve()
        installed = owned / "installed"
        run(["cargo", "install", "--locked", "--offline", "--debug", "--path", ROOT / "crates/edgehome-kit",
             "--root", installed], ROOT)
        developer_cli = installed / "bin" / ("edgehome-dev" + SUFFIX)
        assert BASELINE in run([developer_cli, "--version"], owned).stdout
        print("Installed CLI runs outside the SDK checkout.", flush=True)

        project = owned / "local-agent"
        run([developer_cli, "init", project, "--sdk-path", ROOT, "--offline", "--name", "onboarding-local"], owned)
        exercise(project, "onboarding-local")
        assert run([developer_cli, "init", project], owned, check=False).returncode != 0
        print("Explicit local-development mode passed.", flush=True)

        remote = owned / "sdk-remote"
        # Retain published history so the default compatibility baseline is
        # tested as well as the current working tree, without a network clone.
        run(["git", "clone", "--quiet", "--no-checkout", ROOT, remote], owned)
        paths = run(["git", "ls-files", "--cached", "--others", "--exclude-standard"], ROOT).stdout.splitlines()
        for relative in paths:
            source = ROOT / relative
            if source.is_file():
                destination = remote / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
        hooks = owned / "empty-hooks"
        hooks.mkdir()
        run(["git", "config", "core.hooksPath", hooks.as_posix()], remote)
        first = commit_fixture(remote)
        # Child-process-only rewrite: the manifest always contains the public URL.
        transport = os.environ.copy()
        count = int(transport.get("GIT_CONFIG_COUNT", "0"))
        transport.update({
            "CARGO_NET_GIT_FETCH_WITH_CLI": "true", "CARGO_NET_OFFLINE": "false",
            "CARGO_HTTP_TIMEOUT": "5", "CARGO_NET_RETRY": "0",
            "GIT_CONFIG_COUNT": str(count + 1),
            f"GIT_CONFIG_KEY_{count}": f"url.{remote.as_uri()}.insteadOf",
            f"GIT_CONFIG_VALUE_{count}": REPOSITORY,
        })
        # Fetch this Git revision through init before using cargo install offline.
        # Workspace build above supplies the registry cache; installation must
        # not depend on a fresh crates.io index/yank check for each local fixture.
        seed = owned / "cache-seed"
        seeded = run([developer_cli, "init", seed, "--sdk-rev", first], owned, env=transport, check=False)
        if seeded.returncode:
            # Git transport is local, but Cargo may refresh the registry index.
            # Exercise the documented recovery using the prebuilt registry cache.
            # If Git did not fetch or the manifest is invalid, this still fails.
            run(["cargo", "update", "--workspace", "--offline"], seed)
        git_install = owned / "git-installed"
        run(["cargo", "install", "--locked", "--offline", "--debug", "--path", remote / "crates/edgehome-kit",
             "--root", git_install, "--bin", "edgehome-dev"], remote)
        developer_cli = git_install / "bin" / ("edgehome-dev" + SUFFIX)
        version = run([developer_cli, "--version"], owned).stdout
        assert BASELINE in version, f"installed SDK baseline mismatch: {version}"
        project = owned / "portable-agent"
        run([developer_cli, "init", project, "--offline", "--name", "onboarding-portable"], owned)
        verify_lock(project, BASELINE)
        manifest = (project / "Cargo.toml").read_bytes()
        lock = (project / "Cargo.lock").read_bytes()
        tools = (project / "src/tools.rs").read_bytes()
        # Only temporary directories owned by this test may be moved.
        assert project.parent == remote.parent == owned
        project = project.rename(owned / "moved-agent")
        unavailable = remote.rename(owned / "remote-unavailable")
        exercise(project, "onboarding-portable")
        verify_lock(project, BASELINE)
        (project / "Cargo.lock").unlink()
        assert run(["cargo", "build", "--locked", "--offline"], project, check=False).returncode != 0
        (project / "Cargo.lock").write_bytes(lock)
        print("Git-pinned app relocated; locked offline build, tools, RPC and 8-case gate passed.", flush=True)

        unavailable.rename(remote)
        with (remote / "README.md").open("a", encoding="utf-8") as file:
            file.write("\nOnboarding upgrade fixture.\n")
        # Advance the compatibility baseline explicitly, as a maintainer would
        # after publishing the preceding SDK commit. Rust source stays identical.
        (remote / "crates/edgehome-kit/sdk-revision.txt").write_text(first + "\n", encoding="utf-8")
        second = commit_fixture(remote)
        (project / "Cargo.toml").write_bytes(manifest.replace(BASELINE.encode(), second.encode()))
        updated = run(["cargo", "update", "--workspace"], project, env=transport, check=False)
        if updated.returncode:
            run(["cargo", "update", "--workspace", "--offline"], project)
        # The new CLI must contain the changed, tracked baseline, while leaving
        # the application's independently selected revision untouched.
        app_manifest = (project / "Cargo.toml").read_bytes()
        run(["cargo", "install", "--locked", "--offline", "--debug", "--force", "--path", remote / "crates/edgehome-kit",
             "--root", git_install, "--bin", "edgehome-dev"], remote)
        version = run([developer_cli, "--version"], owned).stdout
        assert first in version, f"upgraded SDK baseline mismatch: expected {first}, got {version}"
        assert (project / "Cargo.toml").read_bytes() == app_manifest
        assert run(["git", "rev-parse", "HEAD"], remote).stdout.strip() == second
        verify_lock(project, second)
        run(["cargo", "build", "--locked", "--offline"], project)
        application = TARGET / "debug" / ("onboarding-portable" + SUFFIX)
        assert first in run([application, "--version"], project).stdout
        (project / "Cargo.toml").write_bytes(manifest)
        (project / "Cargo.lock").write_bytes(lock)
        run(["cargo", "build", "--locked", "--offline"], project)
        verify_lock(project, BASELINE)
        assert (project / "src/tools.rs").read_bytes() == tools
        print("Explicit upgrade and rollback preserve application tools and locked registry versions.", flush=True)

        failed = owned / "unavailable-revision"
        result = run([developer_cli, "init", failed, "--sdk-rev", "0" * 40, "--offline"], owned, check=False)
        assert result.returncode != 0 and "project kept at" in result.stderr
        assert (failed / "src/tools.rs").is_file()
        print("SDK onboarding passed, including recoverable lock failure and overwrite refusal.", flush=True)


if __name__ == "__main__":
    main()
