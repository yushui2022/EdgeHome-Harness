"""Verify a generated app outside the repository, including the Python RPC host."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
os.environ["CARGO_TARGET_DIR"] = str(TARGET)
SUFFIX = ".exe" if os.name == "nt" else ""


def run(arguments, cwd, *, check=True):
    result = subprocess.run(
        [str(arg) for arg in arguments], cwd=cwd, text=True,
        encoding="utf-8", stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=180, check=False,
    )
    if check and result.returncode:
        raise RuntimeError(f"command failed: {arguments}\n{result.stdout}\n{result.stderr}")
    return result


def main():
    run(["cargo", "build", "--locked", "-p", "edgehome-kit", "--bin", "edgehome-dev"], ROOT)
    developer_cli = TARGET / "debug" / ("edgehome-dev" + SUFFIX)
    with tempfile.TemporaryDirectory(prefix="edgehome-onboarding-") as directory:
        owned = Path(directory).resolve()
        assert owned.parent == Path(tempfile.gettempdir()).resolve()
        project = owned / "independent-agent"
        run([developer_cli, "init", project, "--sdk-path", ROOT, "--name", "onboarding-probe"], ROOT)
        run(["cargo", "build", "--offline"], project)
        # Build and run the generated application, not an SDK example binary.
        application = TARGET / "debug" / ("onboarding-probe" + SUFFIX)
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
        assert run([developer_cli, "init", project], ROOT, check=False).returncode != 0
        print("SDK onboarding passed: independent build, two tools, preview/approval, events, Python RPC, 8-case fixture gate, overwrite refusal.")


if __name__ == "__main__":
    main()
