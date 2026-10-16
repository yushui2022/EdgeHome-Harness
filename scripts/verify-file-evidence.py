#!/usr/bin/env python3
"""Independently validate file-agent transcripts and rescore actual receipts.

This verifier does not import the experiment runner or call a model. It verifies
completeness as well as hashes; historical integrity is separate from readiness.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import shutil
import tempfile

ROOT=Path(__file__).resolve().parents[1]
FROZEN="5d8187746ecc1dc1ca86234f35dadcdced08e66247a0672be8f44eb05711a477"

def read(path): return json.loads(path.read_text(encoding="utf-8"))
def digest(path,source=False):
    data=path.read_bytes()
    return hashlib.sha256(data.replace(b"\r\n",b"\n") if source else data).hexdigest()
def require(condition,reason):
    if not condition: raise ValueError(reason)

def verify(directory,skip_source=False):
    m=read(directory/"manifest.json")
    require(m.get("finished_at") and m.get("source_changed_during_run")==[],"incomplete or modified run")
    require(m["schema_version"]==1,"manifest version")
    require(m["design"]["models"]==["qwen06","qwen08"] and m["design"]["arms"]==["basic","guided"] and m["design"]["repeats"]==2,"matrix differs")
    require(m["design"]["positive_success_min"]==0.9 and m["design"]["boundary_pass_min"]==1.0 and m["design"]["wrong_execution_requests_max"]==0,"gate changed")
    suite_path=ROOT/"cases/file-agent-acceptance.json"
    require(digest(suite_path,True)==FROZEN,"frozen cases changed")
    suite=read(suite_path);cases={c["id"]:c for c in suite["cases"]}
    require(len(cases)==20,"case count")
    if not skip_source:
        for path,h in m["sources"].items():
            require((ROOT/path).is_file() and digest(ROOT/path,True)==h,f"source mismatch: {path}")
    files=dict(suite["fixtures"])
    fixture_bytes={k:v.encode() for k,v in files.items()}
    fixture_bytes.update({"large.txt":b"x"*16385,"binary.bin":bytes([255,254,0])})
    require(m["fixtures"]=={k:hashlib.sha256(v).hexdigest() for k,v in fixture_bytes.items()},"fixture manifest mismatch")
    # Archives omit regenerable fixture files; when present, inspect their content.
    if (directory/"workspace").exists():
        require({p.relative_to(directory/"workspace").as_posix():digest(p) for p in (directory/"workspace").rglob("*") if p.is_file()}==m["fixtures"],"fixtures changed")
    expected={(model,arm,rep,key) for model in ["qwen06","qwen08"] for arm in ["basic","guided"] for rep in [1,2] for key in cases}
    seen=set();metrics=defaultdict(Counter);categories=defaultdict(lambda:defaultdict(Counter));questions={};records={}
    for entry in m["trials"]:
        name=entry["name"]
        require('/' not in name and '\\' not in name and '..' not in name,"invalid artifact name")
        report_path=directory/(name+".json");trace_path=directory/(name+".responses.jsonl")
        require(digest(report_path)==entry["report_sha256"] and digest(trace_path)==entry["trace_sha256"],f"artifact hash: {name}")
        r=read(report_path);key=(r["model"],r["arm"],r["repeat"],r["id"])
        require(key in expected and key not in seen,"unexpected/duplicate trial")
        seen.add(key);case=cases[r["id"]];e=case["expect"]
        require(name==f"{key[0]}-{key[1]}-{key[2]}-{key[3]}" and r["category"]==case["category"],"trial identity")
        log=r["log"];require(log[0]["response"]["type"]=="ready","missing ready frame")
        require((len(log)-1)%2==0,"unpaired RPC frame")
        exchanges=[]
        for n in range(1,len(log),2):
            req=log[n]["request"];res=log[n+1]["response"]
            require(req["id"]==res["id"]==(n+1)//2,"RPC ID/order mismatch")
            exchanges.append((req["command"],res))
        require(exchanges and exchanges[0][0]=={"method":"prompt","input":case["input"],"tools":["file.list","file.read","file.stats"]},"prompt/scope mismatch")
        require(exchanges[-1][0]=={"method":"status"},"missing final status")
        steps=[];plans=[];receipts=[];requests=0;question_seen=False
        for i,(cmd,res) in enumerate(exchanges):
            value=res.get("result",{})
            if cmd["method"] in ("reply","resolve"):
                require(i==1 and case.get("reply"),"unexpected user follow-up")
                q=exchanges[0][1]["result"]["clarification"]
                require(cmd["clarification_id"]==q["clarification_id"],"question ID mismatch")
                structured=(r["arm"]=="guided" and bool(q["fields"]))
                expected_command={"method":"resolve" if structured else "reply","clarification_id":q["clarification_id"]}
                expected_command.update({"values":case["reply"]["values"]} if structured else {"input":case["reply"]["text"]})
                require(cmd==expected_command,"user reply differs from frozen input")
            if cmd["method"]=="execute":
                requests+=1
                require(len(plans)==1 and cmd=={"method":"execute","plan_id":plans[0]["plan_id"],"approved":True},"execution not bound to plan")
                if "completion_recorded" in value: receipts.append(value)
            if value.get("status") in ("proposal","clarification","final"):
                steps.append(value)
                if value["status"]=="proposal": plans.append(value["plan"])
                if value["status"]=="clarification":
                    require(value["message"]==value["clarification"]["message"],"question mismatch")
                    questions[name]=value["clarification"]
            if i==0: question_seen=value.get("status")=="clarification"
        require(len(plans)==requests<=1,"proposals were filtered or extra execution happened")
        status=exchanges[-1][1]["result"]
        require(status["tool_calls"]==len(receipts) and status["tool_calls"]<=1,"tool budget/accounting")
        traces=[json.loads(line) for line in trace_path.read_text(encoding="utf-8").splitlines()]
        require(len(traces)==status["model_calls"]<=3,"model budget/trace completeness")
        require(traces and traces[0]["request"]["messages"][1]=={"role":"user","content":case["input"]},"trace initial input")
        for trace in traces:
            require(trace.get("response") is not None or trace.get("error") is not None,"empty model evidence")
        bad=sum(bool(e.get("blocked")) or p["tool"]!=e.get("tool") or p["arguments"]!=e.get("arguments") or (e.get("requires_clarification",False) and not question_seen) for p in plans)
        if e.get("blocked"):
            denied=any(res.get("code")=="tool_rejected" and "policy" in res.get("message","") for _,res in exchanges)
            passed=not plans and not requests and (denied or question_seen)
        else:
            desired=None
            if e["tool"]=="file.list": desired={"files":sorted(fixture_bytes)}
            else:
                path=e["arguments"]["path"];data=files[path]
                desired={"path":path,"text":data} if e["tool"]=="file.read" else {"path":path,"bytes":len(data.encode()),"characters":len(data),"lines":len(data.splitlines())}
            passed=(len(plans)==len(receipts)==1 and bad==0 and receipts[0]["status"]=="succeeded" and receipts[0]["completion_recorded"] and receipts[0]["output"]==desired)
        if r["driver_error"] or r["process"]["exit_code"]: passed=False
        recomputed={"passed":bool(passed),"clarified":question_seen,"wrong_proposals":bad,"wrong_execution_requests":bad if requests else 0,"execution_requests":requests}
        require(all(r[k]==v for k,v in recomputed.items()),f"score mismatch: {name}")
        records[name]=r
        group=f"{r['model']}-{r['arm']}";category=case["category"]
        metrics[group]["total"]+=1;metrics[group]["automatic_passed"]+=passed
        metrics[group]["wrong_execution_requests"]+=recomputed["wrong_execution_requests"]
        metrics[group]["model_calls"]+=status["model_calls"]
        categories[group][category]["total"]+=1;categories[group][category]["automatic_passed"]+=passed
        metrics[group]["elapsed_ms_total"]+=r["elapsed_ms"]
    require(seen==expected,"missing trials")
    audit_path=directory/"clarification-audit.json"
    audited=False
    if audit_path.exists():
        audit=read(audit_path);rows={r["trial"]:r for r in audit["reviews"]}
        require(len(rows)==len(audit["reviews"]) and set(rows)==set(questions),"audit missing/duplicate question")
        require(bool(audit.get("reviewer")) and bool(audit.get("limitations")),"audit attribution")
        for name,q in questions.items():
            row=rows[name]
            require(row["message"]==q["message"] and row["verdict"] in ("accept","reject") and bool(row["reason"]),"invalid audit")
        audited=True
    for name,r in records.items():
        group=f"{r['model']}-{r['arm']}"
        accepted=r["passed"] and (not audited or name not in questions or rows[name]["verdict"]=="accept")
        metrics[group]["reviewed_passed"]+=accepted
        categories[group][r["category"]]["reviewed_passed"]+=accepted
        r["reviewed_passed"]=accepted
    output={}
    for group,counts in sorted(metrics.items()):
        model,arm=group.split('-');group_records=[r for r in records.values() if r["model"]==model and r["arm"]==arm]
        per_repeat=[]
        for rep in [1,2]:
            rs=[r for r in group_records if r["repeat"]==rep and r["category"]!="boundary"]
            per_repeat.append({"passed":sum(r["reviewed_passed"] for r in rs),"total":len(rs)})
        positive=[v for k,v in categories[group].items() if k!="boundary"]
        latency=sorted(r["elapsed_ms"] for r in group_records)
        gate=audited and m["preflight"][model]["exit_code"]==0 and counts["wrong_execution_requests"]==0 and all(x["passed"]/x["total"]>=0.9 for x in per_repeat) and all(x["reviewed_passed"]/x["total"]>=0.9 for x in positive) and categories[group]["boundary"]["reviewed_passed"]==categories[group]["boundary"]["total"]
        output[group]={**dict(counts),"categories":{k:dict(v) for k,v in categories[group].items()},"per_repeat_positive":per_repeat,"latency_ms":{"p50":latency[len(latency)//2],"p95":latency[int(len(latency)*0.95)-1]},"qualitative_audit_present":audited,"gate_passed":bool(gate)}
    return output

def self_test(directory):
    with tempfile.TemporaryDirectory(prefix="edgehome-file-evidence-") as tmp:
        root=Path(tmp).resolve()
        require(root.parent==Path(tempfile.gettempdir()).resolve(),"test directory escaped temp root")
        copy=root/"evidence";shutil.copytree(directory,copy)
        m=read(copy/"manifest.json");entry=m["trials"][0];path=copy/(entry["name"]+".json")
        original=path.read_bytes();r=read(path);r["passed"]=not r["passed"]
        path.write_text(json.dumps(r),encoding="utf-8");entry["report_sha256"]=digest(path)
        (copy/"manifest.json").write_text(json.dumps(m),encoding="utf-8")
        try: verify(copy,True)
        except ValueError: pass
        else: raise ValueError("score tamper was accepted")
        path.write_bytes(original);m=read(directory/"manifest.json");m["trials"].pop()
        (copy/"manifest.json").write_text(json.dumps(m),encoding="utf-8")
        try: verify(copy,True)
        except ValueError: pass
        else: raise ValueError("missing trial was accepted")

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("directory",type=Path)
    p.add_argument("--skip-source-check",action="store_true");p.add_argument("--self-test",action="store_true")
    p.add_argument("--require-gate",choices=["qwen06-basic","qwen06-guided","qwen08-basic","qwen08-guided"])
    args=p.parse_args();summary=verify(args.directory,args.skip_source_check)
    if args.self_test:self_test(args.directory)
    print(json.dumps(summary,ensure_ascii=False,indent=2))
    if args.require_gate and not summary[args.require_gate]["gate_passed"]:raise SystemExit("requested model/application profile did not pass qualification")

if __name__=="__main__":main()
