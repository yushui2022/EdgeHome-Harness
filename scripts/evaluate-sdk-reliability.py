#!/usr/bin/env python3
"""Paired real-model evidence run. No downloads, mock fallback, or success retries."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess
import sys
import time
from datetime import datetime, timezone
from urllib.request import build_opener, ProxyHandler, Request

ROOT = Path(__file__).resolve().parents[1]
def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def source_digest(path):
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
def now():
    return datetime.now(timezone.utc).isoformat()
def api(path, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    req = Request("http://127.0.0.1:11434" + path, data=data, headers={"Content-Type":"application/json"})
    with build_opener(ProxyHandler({})).open(req, timeout=60) as response:
        raw = response.read(2*1024*1024+1)
    if len(raw)>2*1024*1024:
        raise RuntimeError("Ollama metadata response exceeds limit")
    return json.loads(raw)
def invoke(command, timeout):
    run = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    return {"command": list(map(str,command)), "exit_code":run.returncode, "stdout":run.stdout, "stderr":run.stderr}

def gate(report):
    # All thresholds fixed before evaluation; no pooled repeats pretending to be
    # independent samples, and no hiding incorrect calls behind an oracle guard.
    return (
        report["mode"] == "model"
        and all(c["passed"]/c["total"] >= 0.9 for c in report["per_repeat"])
        and all(c["passed"]/c["total"] >= 0.9 for c in report["per_category"].values())
        and report["unexpected_proposals"] == 0
    )

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="New evidence directory")
    args = parser.parse_args()
    out = args.output.resolve()
    if out.exists():
        raise SystemExit("Refusing to overwrite an evidence directory")
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT/"target")).resolve()
    build = invoke(["cargo","build","--locked","--offline","-p","edgehome-kit","--example","reliability_lab"], 300)
    if build["exit_code"]:
        raise SystemExit(build["stderr"])
    binary = target/"debug"/"examples"/("reliability_lab.exe" if os.name=="nt" else "reliability_lab")
    out.mkdir(parents=True)
    save(out/"build.json", build)
    sources = [ROOT/"Cargo.lock", ROOT/"Cargo.toml"]
    for directory in ["crates/edgehome-runtime/src","crates/edgehome-inference/src","crates/edgehome-kit/src","crates/edgehome-kit/templates","crates/edgehome-kit/examples","configs/reliability"]:
        sources.extend(p for p in (ROOT/directory).rglob("*") if p.is_file())
    sources.extend([ROOT/"cases/sdk-starter.json",ROOT/"cases/sdk-reliability-holdout.json",Path(__file__).resolve()])
    manifest = {
        "schema_version":1, "started_at":now(), "git_base":invoke(["git","rev-parse","HEAD"],10)["stdout"].strip(),
        "source_state":"working_tree", "source_hash_encoding":"UTF-8 bytes with CRLF normalized to LF",
        "sources":{str(p.relative_to(ROOT)).replace("\\","/"):source_digest(p) for p in sorted(set(sources))},
        "binary_sha256":digest(binary), "platform":platform.platform(), "processor":platform.processor(),
        "python":sys.version, "rustc":invoke(["rustc","-Vv"],10)["stdout"], "ollama_version":api("/api/version"),
        "design":{"development_cases":8,"holdout_cases":36,"repetitions":3,"order_seed":42,
            "per_repeat_success_min":0.9,"per_category_success_min":0.9,"unexpected_proposals_max":0,
            "execution":"All allowlisted proposals execute in side-effect-free test tools before scoring; no oracle in runtime.",
            "scope":"One decision and tool receipt or a relevant clarification; no final-answer or long-horizon claim.",
            "warmup":"Two unscored system-role probes per profile; no per-case retry outside bounded runtime repairs.",
            "statistics":"Temperature zero repetitions measure consistency, not independent samples or statistical significance."},
        "models":{}, "results":[]
    }
    tags = api("/api/tags")["models"]
    for name, model in [("qwen06","edgehome-qwen3-06b-chat:v1"),("qwen08","qwen3.5:0.8b"),("minicpm","openbmb/minicpm5:latest")]:
        show = api("/api/show", {"model":model})
        manifest["models"][name] = {
            "name":model, "digest":next(m["digest"] for m in tags if m["name"]==model),
            "template":show.get("template"),"parameters":show.get("parameters"),"details":show.get("details"),
            "capabilities":show.get("capabilities"),
            "renderer_parser":[s for s in show.get("modelfile","").splitlines() if s.startswith(("RENDERER ","PARSER "))],
        }
    # The manifest, suites, source and thresholds exist before the first scored call.
    save(out/"manifest.json",manifest)
    for model in ["qwen06","qwen08","minicpm"]:
        for protocol in ["tagged","action"]:
            config = ROOT/"configs"/"reliability"/f"{model}-{protocol}.yaml"
            warmup = invoke([str(binary),"doctor","--config",str(config),"--probe"],150)
            save(out/f"{model}-{protocol}-preflight.json",warmup)
            for suite, path in [("development","sdk-starter.json"),("holdout","sdk-reliability-holdout.json")]:
                stem = f"{model}-{protocol}-{suite}"
                command = [str(binary),"eval","--config",str(config),"--cases",str(ROOT/"cases"/path),
                    "--repeat","3","--order-seed","42","--include-responses",
                    "--allow","document.stats","--allow","sensor.read","--allow","length.convert"]
                started = now()
                result = invoke(command, 15000)
                save(out/f"{stem}-process.json",result)
                report = json.loads(result["stdout"])
                # Process errors/missing cases are never interpreted as accuracy.
                expected = (8 if suite=="development" else 36)*3
                if report["total"] != expected or result["exit_code"] not in (0,1):
                    raise RuntimeError(f"incomplete evaluation: {stem}")
                if (result["exit_code"]==0) != (report["passed"]==report["total"]):
                    raise RuntimeError(f"exit status/report mismatch: {stem}")
                save(out/f"{stem}.json",report)
                passed_gate = warmup["exit_code"]==0 and gate(report)
                entry = {"name":stem,"started_at":started,"finished_at":now(),"passed":report["passed"],"total":report["total"],
                    "preflight_passed":warmup["exit_code"]==0,"gate_passed":passed_gate,"report_sha256":digest(out/f"{stem}.json")}
                manifest["results"].append(entry)
                save(out/"manifest.json",manifest)
                print(json.dumps(entry), flush=True)
    changed = [path for path, expected in manifest["sources"].items() if source_digest(ROOT/path)!=expected]
    manifest["source_changed_during_run"] = changed
    manifest["finished_at"] = now()
    save(out/"manifest.json",manifest)
    if changed:
        raise SystemExit("Source changed during evaluation; evidence is invalid")
    print("Evidence complete. Gate failures remain in the report; they are not retried.")

if __name__ == "__main__":
    main()
