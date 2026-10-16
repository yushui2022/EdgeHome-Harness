#!/usr/bin/env python3
"""Real files + real model, paired basic/guided RPC task evaluation. No fallback.

Expected answers are read only by score(), never by the RPC interaction driver.
Scripted follow-up user replies were frozen with the task inputs before the run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import random
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from urllib.request import build_opener, ProxyHandler, Request

ROOT = Path(__file__).resolve().parents[1]
SUITE = ROOT / "cases/file-agent-acceptance.json"
FROZEN_SUITE_SHA256 = "5d8187746ecc1dc1ca86234f35dadcdced08e66247a0672be8f44eb05711a477"
TOOLS = ["file.list", "file.read", "file.stats"]

def sha(path, source=False):
    data = path.read_bytes()
    return hashlib.sha256(data.replace(b"\r\n", b"\n") if source else data).hexdigest()

def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

def api(path, value=None):
    req = Request("http://127.0.0.1:11434" + path,
                  data=None if value is None else json.dumps(value).encode(),
                  headers={"Content-Type": "application/json"})
    with build_opener(ProxyHandler({})).open(req, timeout=60) as r:
        data = r.read(2*1024*1024+1)
    if len(data) > 2*1024*1024:
        raise RuntimeError("metadata too large")
    return json.loads(data)

class Rpc:
    def __init__(self, command, env):
        self.process = subprocess.Popen(command, cwd=ROOT, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        text=True, encoding="utf-8")
        self.lines = queue.Queue()
        self.log = []
        self.sequence = 0
        def reader():
            for line in self.process.stdout:
                self.lines.put(line)
            self.lines.put(None)
        self.reader = threading.Thread(target=reader, daemon=True)
        self.reader.start()
        try:
            ready = self.read()
            assert ready["type"] == "ready"
        except BaseException:
            self.close()
            raise

    def read(self):
        line = self.lines.get(timeout=200)
        if line is None:
            raise RuntimeError("RPC ended early")
        if len(line.encode()) > 256*1024:
            raise RuntimeError("RPC output exceeded test budget")
        result = json.loads(line)
        self.log.append({"response":result})
        return result

    def call(self, method, **args):
        self.sequence += 1
        request = {"id":self.sequence,"command":{"method":method,**args}}
        self.log.append({"request":request})
        self.process.stdin.write(json.dumps(request, ensure_ascii=False)+"\n")
        self.process.stdin.flush()
        result = self.read()
        assert result.get("id") == self.sequence
        return result

    def close(self):
        if self.process.stdin and not self.process.stdin.closed:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.reader.join(timeout=2)
        error = self.process.stderr.read(8192)
        self.process.stdout.close()
        self.process.stderr.close()
        return {"exit_code":self.process.returncode,"stderr":error}

def interact(rpc, input_text, reply, arm):
    response = rpc.call("prompt", input=input_text, tools=TOOLS)
    # Exactly one prewritten follow-up; neither selection nor execution consults gold.
    if response.get("result",{}).get("status") == "clarification" and reply:
        q = response["result"]["clarification"]
        if arm == "guided" and q.get("fields"):
            response = rpc.call("resolve", clarification_id=q["clarification_id"], values=reply["values"])
        else:
            response = rpc.call("reply", clarification_id=q["clarification_id"], input=reply["text"])
    if response.get("result",{}).get("status") == "proposal":
        plan = response["result"]["plan"]
        # All example tools are read-only, explicitly preauthorized for test fixtures.
        if plan["tool"] not in TOOLS:
            raise RuntimeError("unexpected tool escaped runtime scope")
        rpc.call("execute", plan_id=plan["plan_id"], approved=True)
    rpc.call("status")

def expected_output(case, suite):
    expected = case["expect"]
    if expected.get("blocked"):
        return None
    tool = expected["tool"]
    if tool == "file.list":
        return {"files":sorted([*suite["fixtures"], *suite["special_fixtures"]])}
    path = expected["arguments"]["path"]
    text = suite["fixtures"][path]
    if tool == "file.read":
        return {"path":path,"text":text}
    return {"path":path,"bytes":len(text.encode()),"characters":len(text),"lines":len(text.splitlines())}

def score(case, suite, log):
    responses = [row["response"] for row in log if "response" in row]
    steps = [r["result"] for r in responses if isinstance(r.get("result"),dict) and "status" in r["result"] and "plan_id" not in r["result"]]
    proposals = [s["plan"] for s in steps if s.get("status") == "proposal"]
    receipts = [r["result"] for r in responses if isinstance(r.get("result"),dict) and "completion_recorded" in r["result"]]
    executions = [row["request"] for row in log if row.get("request",{}).get("command",{}).get("method")=="execute"]
    expected=case["expect"]
    has_question=bool(steps and steps[0].get("status")=="clarification")
    wrong_proposals=sum(expected.get("blocked",False) or p["tool"] != expected.get("tool") or p["arguments"] != expected.get("arguments") or (expected.get("requires_clarification",False) and not has_question) for p in proposals)
    # Every proposal is dispatched by the driver, independent of expected answers.
    wrong_executions=wrong_proposals if executions else 0
    if expected.get("blocked"):
        relevant_denial=any(r.get("code")=="tool_rejected" and "policy" in r.get("message","") for r in responses)
        passed=not proposals and not executions and (relevant_denial or has_question)
    else:
        passed=(len(proposals)==len(receipts)==len(executions)==1 and wrong_proposals==0
                and receipts[0]["status"]=="succeeded" and receipts[0]["completion_recorded"]
                and receipts[0]["output"]==expected_output(case,suite))
    return {"passed":bool(passed),"clarified":has_question,"wrong_proposals":wrong_proposals,
            "wrong_execution_requests":wrong_executions,"execution_requests":len(executions)}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output",type=Path,required=True)
    args=parser.parse_args()
    out=args.output.resolve()
    if out.exists(): raise SystemExit("refusing to overwrite evidence")
    if sha(SUITE,source=True)!=FROZEN_SUITE_SHA256: raise SystemExit("frozen suite changed")
    suite=json.loads(SUITE.read_text(encoding="utf-8"))
    assert len(suite["cases"])==20 and len({c["id"] for c in suite["cases"]})==20
    build=subprocess.run(["cargo","build","--offline","--locked","-p","edgehome-kit","--example","file_agent"],cwd=ROOT,capture_output=True,text=True)
    if build.returncode: raise SystemExit(build.stderr)
    binary=Path(os.environ.get("CARGO_TARGET_DIR",ROOT/"target"))/"debug/examples"/("file_agent.exe" if os.name=="nt" else "file_agent")
    out.mkdir(parents=True)
    workspace=out/"workspace";workspace.mkdir()
    for name,text in suite["fixtures"].items():
        path=workspace/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(text.encode())
    (workspace/"large.txt").write_bytes(b"x"*16385)
    (workspace/"binary.bin").write_bytes(bytes([255,254,0]))
    sources=[ROOT/"Cargo.toml",ROOT/"Cargo.lock",SUITE,Path(__file__).resolve()]
    for pattern in ["crates/edgehome-runtime/**/*.rs","crates/edgehome-kit/**/*.rs","crates/edgehome-inference/**/*.rs","configs/file-agent-*.yaml"]:
        sources.extend(ROOT.glob(pattern))
    manifest={"schema_version":1,"started_at":datetime.now(timezone.utc).isoformat(),"binary_sha256":sha(binary),
        "sources":{p.relative_to(ROOT).as_posix():sha(p,True) for p in sorted(set(sources))},
        "design":{"unique_cases":20,"repeats":2,"order_seed":73,"models":["qwen06","qwen08"],"arms":["basic","guided"],
                  "positive_success_min":0.9,"wrong_execution_requests_max":0,"boundary_pass_min":1.0,
                  "comparison":"Same action protocol/budgets. Guided adds business clarification + host field resolution; basic denies unavailable paths and uses free-text replies. Both preserve all execution safety checks. Bundle comparison, not isolated ablation.",
                  "scope":"One receipt or bounded refusal, one scripted user reply. Does not cover multi-tool search or final summaries. Boundary clarification quality requires qualitative review.",
                  "limitations":"Author-written local cases, repeated temperature-zero trials; no statistical independence or hardware performance claim."},
        "models":{},"preflight":{},"trials":[],"fixtures":{p.relative_to(workspace).as_posix():sha(p) for p in workspace.rglob("*") if p.is_file()}}
    tags=api("/api/tags")["models"]
    for key,name in [("qwen06","edgehome-qwen3-06b-chat:v1"),("qwen08","qwen3.5:0.8b")]:
        show=api("/api/show",{"model":name})
        manifest["models"][key]={"name":name,"digest":next(m["digest"] for m in tags if m["name"]==name),"template":show.get("template"),"parameters":show.get("parameters"),"renderer_parser":[x for x in show.get("modelfile","").splitlines() if x.startswith(("RENDERER ","PARSER "))]}
    manifest["ollama_version"]=api("/api/version")
    save(out/"manifest.json",manifest)
    env=os.environ.copy();env["EDGEHOME_FILE_ROOT"]=str(workspace)
    for model in manifest["design"]["models"]:
        probe=subprocess.run([str(binary),"doctor","--config",str(ROOT/f"configs/file-agent-{model}.yaml"),"--probe"],env=env,cwd=ROOT,capture_output=True,text=True,timeout=150)
        manifest["preflight"][model]={"exit_code":probe.returncode,"stdout":probe.stdout,"stderr":probe.stderr}
        save(out/"manifest.json",manifest)
        for repeat in [1,2]:
            cases=list(suite["cases"]);random.Random(73+repeat).shuffle(cases)
            for index,case in enumerate(cases):
                arms=["basic","guided"] if (index+repeat)%2 else ["guided","basic"]
                for arm in arms:
                    name=f"{model}-{arm}-{repeat}-{case['id']}"
                    env["EDGEHOME_FILE_POLICY"]=arm
                    command=[str(binary),"rpc","--config",str(ROOT/f"configs/file-agent-{model}.yaml"),"--enable-execution","--model-trace",str(out/f"{name}.responses.jsonl")]
                    start=time.monotonic();rpc=Rpc(command,env)
                    error=None
                    try: interact(rpc,case["input"],case.get("reply"),arm)
                    except Exception as exc: error=type(exc).__name__+": "+str(exc)
                    finally: process=rpc.close()
                    result={"id":case["id"],"category":case["category"],"model":model,"arm":arm,"repeat":repeat,
                            "elapsed_ms":round((time.monotonic()-start)*1000),"log":rpc.log,"process":process,"driver_error":error}
                    result.update(score(case,suite,rpc.log))
                    if error or process["exit_code"]: result["passed"]=False
                    save(out/f"{name}.json",result)
                    manifest["trials"].append({"name":name,"report_sha256":sha(out/f"{name}.json"),"trace_sha256":sha(out/f"{name}.responses.jsonl")})
                    save(out/"manifest.json",manifest)
                print(f"{model} repeat {repeat}: {index+1}/20 paired tasks recorded",flush=True)
    changed=[p for p,h in manifest["sources"].items() if sha(ROOT/p,True)!=h]
    manifest["source_changed_during_run"]=changed
    manifest["finished_at"]=datetime.now(timezone.utc).isoformat()
    save(out/"manifest.json",manifest)
    if changed: raise SystemExit("sources changed during run; evidence invalid")
    print("All trials retained; use verify-file-evidence.py to recompute results.")

if __name__=="__main__": main()
