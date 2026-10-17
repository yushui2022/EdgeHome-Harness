"""Versioned CLI packaging, extracted-binary smoke tests and release assembly.

Python 3.11+, standard library only. No tool installation or Git mutations.
Cargo may fetch locked dependency sources to collect their license texts.
Artifacts go to an explicit new directory. Tag builds reject dirty source and
version mismatches; local rehearsals must explicitly opt into dirty source.
"""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = json.loads((ROOT / "configs/distribution.json").read_text(encoding="utf-8"))["targets"]


def run(args, cwd=ROOT, **kwargs):
    return subprocess.run(
        [str(arg) for arg in args], cwd=cwd, check=True, text=True,
        encoding="utf-8", capture_output=True, timeout=180, **kwargs,
    ).stdout.strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def version_info(tag=None):
    version = tomllib.loads((ROOT / "crates/edgehome-kit/Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("unsupported CLI release version")
    expected = "v" + version
    if tag is not None and tag != expected:
        raise ValueError(f"tag {tag!r} must match edgehome-kit version {expected}")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    if not any(p["name"] == "edgehome-kit" and p["version"] == version and "source" not in p for p in lock):
        raise ValueError("Cargo.lock does not match the CLI version")
    sdk = (ROOT / "crates/edgehome-kit/sdk-revision.txt").read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"[0-9a-f]{40}", sdk):
        raise ValueError("default SDK baseline must be a full commit")
    return {"version": version, "tag": expected, "sdk_revision": sdk}


def source_info(allow_dirty=False):
    dirty = bool(run(["git", "status", "--porcelain", "--untracked-files=normal"]))
    if dirty and not allow_dirty:
        raise ValueError("release source must be clean; --allow-dirty is only for local rehearsal")
    return {"source_commit": run(["git", "rev-parse", "HEAD"]), "source_dirty": dirty}


def archive_bytes(files, extension):
    """Normalize entry order, dates and permissions (not compiler output)."""
    output = io.BytesIO()
    if extension == "zip":
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name, (data, mode) in sorted(files.items()):
                entry = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
                entry.create_system = 3
                entry.external_attr = (0o100000 | mode) << 16
                archive.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED)
    else:
        with gzip.GzipFile(fileobj=output, mode="wb", mtime=0, filename="") as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for name, (data, mode) in sorted(files.items()):
                    entry = tarfile.TarInfo(name)
                    entry.size, entry.mode, entry.mtime = len(data), mode, 0
                    archive.addfile(entry, io.BytesIO(data))
    return output.getvalue()


def unpack(archive_path, destination):
    """Extract only ordinary files to a fresh directory, without archive paths."""
    contents = {}
    if archive_path.name.endswith(".zip"):
        with zipfile.ZipFile(archive_path) as archive:
            for member in archive.infolist():
                if member.is_dir() or (member.external_attr >> 16) & 0o170000 == 0o120000:
                    raise ValueError("unexpected directory or symlink in CLI archive")
                if member.filename in contents:
                    raise ValueError("duplicate archive member")
                contents[member.filename] = (archive.read(member), member.external_attr >> 16 & 0o777)
    else:
        with tarfile.open(archive_path, "r:gz") as archive:
            for member in archive.getmembers():
                if not member.isfile() or member.name in contents:
                    raise ValueError("only unique regular files are allowed in a CLI archive")
                contents[member.name] = (archive.extractfile(member).read(), member.mode)
    for name in contents:
        path = PurePosixPath(name)
        if path.is_absolute() or ".." in path.parts or "\\" in name or ":" in name or str(path) != name:
            raise ValueError("unsafe archive path")
    destination.mkdir(parents=True, exist_ok=False)
    for name, (data, mode) in contents.items():
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        path.chmod(mode)
    return destination


def verify_package(directory):
    metadata = json.loads((directory / "release.json").read_text(encoding="utf-8"))
    actual = {p.relative_to(directory).as_posix(): digest(p.read_bytes())
              for p in directory.rglob("*") if p.is_file() and p != directory / "release.json"}
    if actual != metadata["files"]:
        raise ValueError("package file inventory or SHA-256 mismatch")
    return metadata


def smoke(directory):
    metadata = verify_package(directory)
    binary = directory / metadata["binary"]
    # No cargo, git, inference service or original source directory on PATH.
    env = os.environ.copy()
    env["PATH"] = ""
    command = lambda *args: run([binary, *args], cwd=directory, env=env)
    version = command("--version")
    if version.splitlines()[0] != f"edgehome-kit {metadata['version']}" and version.splitlines()[0] != f"edgehome-dev {metadata['version']}":
        raise ValueError(f"packaged binary version mismatch: {version}")
    if metadata["sdk_revision"] not in version:
        raise ValueError("packaged binary SDK baseline mismatch")
    def require(condition, message):
        if not condition:
            raise ValueError(f"extracted CLI check failed: {message}")

    require(json.loads(command("doctor", "--mock"))["status"] == "ready", "doctor readiness")
    names = {tool["name"] for tool in json.loads(command("tools"))["tools"]}
    require(names == {"document.stats", "sensor.read"}, "starter tool inventory")
    for tool, prompt, expected in [
        ("document.stats", "Count the sample document", {"characters": 84, "lines": 2}),
        ("sensor.read", "Read the room sensor", {"temperature_c": 22.5, "source": "fixture"}),
    ]:
        args = ["run", "--mock", "--prompt", prompt, "--tool", tool]
        preview = [json.loads(line) for line in command(*args).splitlines()]
        require(len(preview) == 1 and preview[0]["step"]["status"] == "proposal", f"{tool} preview")
        result = [json.loads(line) for line in command(*args, "--allow", tool).splitlines()]
        require(result[-1]["receipt"]["output"] == expected, f"{tool} receipt")
    report = json.loads(command("eval", "--mock", "--cases", "cases.json",
                                "--allow", "document.stats", "--allow", "sensor.read"))
    require(report["passed"] == report["total"] == 8, "8-case gate")
    frames = '\n'.join(json.dumps({"id": i, "command": {"method": method}})
                       for i, method in enumerate(["tools", "status", "cancel"], 1)) + '\n'
    rpc = run([binary, "rpc", "--mock"], cwd=directory, env=env, input=frames)
    messages = [json.loads(line) for line in rpc.splitlines()]
    require(messages[0]["protocol"] == 1, "RPC handshake")
    require([m["id"] for m in messages[1:]] == [1, 2, 3], "RPC request correlation")
    require(all(m["type"] == "result" for m in messages[1:]), "RPC results")
    print(f"Extracted {metadata['target']}: version, tools, doctor, preview, receipts, 8 cases and RPC passed.")


def dependency_notices(target):
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", target]))
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    pending = [p["id"] for p in metadata["packages"] if p["name"] == "edgehome-kit" and p["source"] is None]
    visited, sections = set(), []
    while pending:
        package_id = pending.pop()
        if package_id in visited:
            continue
        visited.add(package_id)
        package = packages[package_id]
        pending.extend(d["pkg"] for d in nodes[package_id]["deps"]
                       if any(k["kind"] != "dev" for k in d["dep_kinds"]))
        if package["source"] is None:
            continue
        base = Path(package["manifest_path"]).parent
        texts = []
        candidates = {p for p in base.rglob("*") if p.is_file()
                      and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE"))}
        if package.get("license_file"):
            candidates.add(base / package["license_file"])
        for path in sorted(candidates):
            if path.is_relative_to(base):
                texts.append(f"--- {path.relative_to(base).as_posix()} ---\n{path.read_text(encoding='utf-8', errors='replace')}")
        if not texts:
            raise ValueError(f"no bundled license text for dependency {package['name']}")
        sections.append((package["name"], f"## {package['name']} {package['version']}\n\nSPDX: {package.get('license')}\nSource: {package['source']}\n\n" + '\n'.join(texts)))
    return ("# Third-party notices\n\nResolved CLI dependencies (including build dependencies).\n\n" + '\n\n'.join(text for _, text in sorted(sections))).encode()


def package(args):
    metadata = {"schema_version": 1, **version_info(args.tag), **source_info(args.allow_dirty)}
    spec = next((t for t in TARGETS if t["target"] == args.target), None)
    if spec is None:
        if not args.allow_dirty or args.target != "x86_64-pc-windows-gnu":
            raise ValueError("target not in the release matrix")
        spec = {"target": args.target, "archive": "zip", "tested_on": "local Windows GNU rehearsal only"}
    host = next(line.removeprefix("host: ") for line in run(["rustc", "-vV"]).splitlines() if line.startswith("host: "))
    if host != args.target:
        raise ValueError("release packages must be built and smoke-tested on their native target")
    metadata.update({"target": args.target, "tested_on": spec["tested_on"],
                     "release_eligible": not metadata["source_dirty"] and spec in TARGETS,
                     "rustc": run(["rustc", "--version"]), "rustflags": os.environ.get("RUSTFLAGS", ""),
                     "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes())})
    if args.tag:
        tagged = run(["git", "rev-parse", f"refs/tags/{args.tag}^{{commit}}"])
        if tagged != metadata["source_commit"]:
            raise ValueError("tag must point at the packaged source commit")
    binary_name = "edgehome-dev.exe" if "windows" in args.target else "edgehome-dev"
    metadata["binary"] = binary_name
    files = {binary_name: (args.binary.read_bytes(), 0o755)}
    for source, destination in [
        ("LICENSE", "LICENSE"), ("docs/cli-distribution.md", "README.md"),
        ("crates/edgehome-kit/templates/agent.yaml", "agent.yaml"),
        ("cases/sdk-starter.json", "cases.json"),
        ("examples/python/agent_client.py", "client.py"),
    ]:
        files[destination] = ((ROOT / source).read_bytes(), 0o644)
    files["THIRD-PARTY-NOTICES.txt"] = (dependency_notices(args.target), 0o644)
    metadata["files"] = {name: digest(data) for name, (data, _) in files.items()}
    files["release.json"] = ((json.dumps(metadata, indent=2, sort_keys=True) + '\n').encode(), 0o644)
    data = archive_bytes(files, spec["archive"])
    name = f"edgehome-dev-{metadata['tag']}-{args.target}.{spec['archive']}"
    args.output.mkdir(parents=True, exist_ok=False)
    archive = args.output / name
    archive.write_bytes(data)
    (args.output / f"{name}.sha256").write_text(f"{digest(data)}  {name}\n", encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="edgehome-release-") as temp:
        smoke(unpack(archive, Path(temp) / "unpacked"))
    print(archive)


def assemble(directory, tag):
    expected = version_info(tag)
    inventories, checksums = [], []
    names = {f"edgehome-dev-{tag}-{t['target']}.{t['archive']}" for t in TARGETS}
    actual = {p.name for p in directory.iterdir()}
    if actual != names | {name + ".sha256" for name in names}:
        raise ValueError("release must contain exactly one archive and checksum for every target")
    for name in sorted(names):
        archive = directory / name
        line = f"{digest(archive.read_bytes())}  {name}\n"
        if (directory / (name + ".sha256")).read_text(encoding="utf-8") != line:
            raise ValueError(f"archive checksum mismatch: {name}")
        with tempfile.TemporaryDirectory(prefix="edgehome-verify-release-") as temp:
            meta = verify_package(unpack(archive, Path(temp) / "unpacked"))
        if not meta["release_eligible"] or meta["source_dirty"] or any(meta[k] != expected[k] for k in expected):
            raise ValueError("inconsistent or non-release package metadata")
        if name != f"edgehome-dev-{tag}-{meta['target']}.{next(t['archive'] for t in TARGETS if t['target'] == meta['target'])}":
            raise ValueError("archive target does not match manifest")
        if meta["source_commit"] != run(["git", "rev-parse", "HEAD"]):
            raise ValueError("package source does not match release checkout")
        inventories.append({"archive": name, "sha256": digest(archive.read_bytes()), **meta})
        checksums.append(line)
    (directory / "SHA256SUMS").write_text(''.join(checksums), encoding="utf-8")
    (directory / "release-manifest.json").write_text(json.dumps(inventories, indent=2) + '\n', encoding="utf-8")
    notes = f"""EdgeHome CLI {tag} — developer preview

Download the archive for your OS/CPU and its `.sha256` file (or `SHA256SUMS`).
Verify before extraction. Each archive includes CLI, fixtures, MIT license,
dependency notices, and `release.json` with file hashes and source/SDK commits.

No Rust installation is needed for `doctor --mock`, fixture `run`, `eval`, or `rpc`.
`init` and custom Rust tools still require Git, Rust 1.95.0 and a native linker.
Models and inference engines are installed separately. The CLI includes starter
document/sensor tools; it does not load arbitrary application tools dynamically.

| Platform | Archive | Build/smoke environment |
| --- | --- | --- |
"""
    for spec in TARGETS:
        notes += f"| {spec['platform']} | `edgehome-dev-{tag}-{spec['target']}.{spec['archive']}` | {spec['tested_on']} |\n"
    notes += f"\nSource: `{inventories[0]['source_commit']}`\n\nDefault SDK: `{expected['sdk_revision']}`\n\n"
    notes += "All native package smoke checks must pass before this release is created.\nThese checks do not qualify real-model task accuracy or physical edge hardware.\n\n"
    notes += f"[Download, verification, upgrade and rollback guide](https://github.com/yushui2022/EdgeHome-Harness/blob/{tag}/docs/cli-distribution.md).\n"
    (directory / "release-notes.md").write_text(notes, encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan = commands.add_parser("plan")
    plan.add_argument("--tag")
    plan.add_argument("--github-output", type=Path)
    pack = commands.add_parser("package")
    pack.add_argument("--tag")
    pack.add_argument("--target", required=True)
    pack.add_argument("--binary", type=Path, required=True)
    pack.add_argument("--output", type=Path, required=True)
    pack.add_argument("--allow-dirty", action="store_true")
    bundle = commands.add_parser("assemble")
    bundle.add_argument("--directory", type=Path, required=True)
    bundle.add_argument("--tag", required=True)
    args = parser.parse_args()
    if args.command == "plan":
        info = version_info(args.tag)
        print(json.dumps({**info, "targets": TARGETS}, indent=2))
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as output:
                output.write(f"tag={info['tag']}\nmatrix={json.dumps({'include': TARGETS})}\n")
    elif args.command == "package":
        package(args)
    else:
        assemble(args.directory, args.tag)


if __name__ == "__main__":
    main()
