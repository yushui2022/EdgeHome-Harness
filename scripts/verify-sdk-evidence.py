#!/usr/bin/env python3
"""Independently rescore saved SDK evidence; never calls a model."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
def read(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def same(a,b):
    # Python bool is a subclass of int, but JSON booleans are not numbers.
    if isinstance(a,bool) or isinstance(b,bool):
        return type(a) is type(b) and a==b
    if isinstance(a,dict) and isinstance(b,dict):
        return a.keys()==b.keys() and all(same(a[k],b[k]) for k in a)
    if isinstance(a,list) and isinstance(b,list):
        return len(a)==len(b) and all(same(x,y) for x,y in zip(a,b))
    if isinstance(a,(int,float)) and isinstance(b,(int,float)):
        return a==b
    return type(a) is type(b) and a==b

def score(case, actual):
    expected=case["expect"]
    step=actual.get("step",{})
    if not step:
        return actual["error"]+": "+actual["message"],False
    if step["status"]=="proposal":
        plan=step["plan"]
        if expected["kind"]=="clarify":
            return "unexpected_proposal",True
        if plan["tool"]!=expected["tool"]:
            return "incorrect_tool",True
        if not same(plan["arguments"],expected["arguments"]):
            return "incorrect_arguments",True
        receipt=actual.get("execution")
        if receipt is None:
            return "execution_not_authorized",False
        if "error" in receipt:
            return receipt["error"],False
        if receipt["status"]!="succeeded" or not receipt["completion_recorded"]:
            return "execution_uncertain",False
        return ("passed" if same(receipt["output"],expected["output"]) else "incorrect_output"),False
    if step["status"]=="clarification":
        if expected["kind"]!="clarify":
            return "unexpected_clarification",False
        terms=expected.get("message_contains_any",[])
        return ("passed" if not terms or any(t.lower() in step["message"].lower() for t in terms) else "irrelevant_clarification"),False
    return "unexpected_decision",False

def verify(directory, check_sources=True):
    manifest=read(directory/"manifest.json")
    assert not manifest.get("source_changed_during_run"),"source changed during run"
    assert manifest.get("finished_at"),"unfinished run"
    assert len(manifest["results"])==12,"missing model/profile/suite reports"
    if check_sources:
        for rel,expected in manifest["sources"].items():
            data=(ROOT/rel).read_bytes().replace(b"\r\n",b"\n")
            assert hashlib.sha256(data).hexdigest()==expected, f"source mismatch: {rel}"
    names=set()
    for result in manifest["results"]:
        name=result["name"]
        assert name not in names
        names.add(name)
        path=directory/(name+".json")
        assert sha(path)==result["report_sha256"],f"hash mismatch: {name}"
        report=read(path)
        suite_name="sdk-reliability-holdout.json" if name.endswith("holdout") else "sdk-starter.json"
        suite=read(ROOT/"cases"/suite_name)
        cases={c["id"]:c for c in suite["cases"]}
        assert report["mode"]=="model" and report["schema_version"]==2
        assert report["repeat"]==3 and report["order_seed"]==42
        assert report["unique_cases"]==len(cases)
        assert report["total"]==len(cases)*3==len(report["cases"])
        model_id,protocol,_=name.split("-")
        assert report["model"]==manifest["models"][model_id]["name"]
        assert report["runtime_configuration"]["decision_protocol"]==protocol
        occurrences=Counter()
        passed=first=recovered=unexpected=0
        per_repeat=[Counter() for _ in range(3)]
        per_category={}
        stable={key:True for key in cases}
        for row in report["cases"]:
            case=cases[row["id"]]
            assert row["category"]==case.get("category","general")
            assert row["repetition"] in [1,2,3]
            occurrences[row["id"],row["repetition"]]+=1
            outcome,wrong=score(case,row["observed"])
            assert row["outcome"]==outcome,(name,row["id"],row["outcome"],outcome)
            assert row["unexpected_proposal"]==wrong
            assert len(row["responses"])==row["model_calls"]
            assert row["model_calls"]<=2 and row["tool_calls"]<=1
            if row["observed"].get("step",{}).get("status")=="proposal":
                assert "execution" in row["observed"],"oracle suppressed an allowed call"
            ok=outcome=="passed"
            passed+=ok
            first+=ok and row["model_calls"]==1
            recovered+=ok and row["model_calls"]>1
            unexpected+=wrong
            stable[row["id"]] &= ok
            for count in [per_repeat[row["repetition"]-1],per_category.setdefault(row["category"],Counter())]:
                count["total"]+=1
                count["passed"]+=ok
        assert set(occurrences)=={(key,r) for key in cases for r in [1,2,3]}
        assert all(n==1 for n in occurrences.values()),"duplicate or omitted trial"
        assert report["passed"]==passed==result["passed"]
        assert report["first_attempt_passed"]==first
        assert report["recovered_after_validation_repair"]==recovered
        assert report["unexpected_proposals"]==unexpected
        assert report["cases_passing_all_repeats"]==sum(stable.values())
        assert report["per_repeat"]==per_repeat and report["per_category"]==per_category
        assert abs(report["task_success_rate"]-passed/report["total"])<1e-12
        latencies=sorted(c["elapsed_ms"] for c in report["cases"])
        total=len(latencies)
        assert report["latency_ms"]=={"p50":latencies[(total-1)//2],"p95":latencies[(total*95+99)//100-1],"max":latencies[-1]}
        preflight=read(directory/(model_id+"-"+protocol+"-preflight.json"))
        assert result["preflight_passed"]==(preflight["exit_code"]==0)
        passed_gate=result["preflight_passed"] and unexpected==0 and all(c["passed"]/c["total"]>=0.9 for c in per_repeat+list(per_category.values()))
        assert result["gate_passed"]==passed_gate
    audit_path=directory/"clarification-audit.json"
    if audit_path.exists():
        audit=read(audit_path)
        expected={}
        reports={}
        for name in names:
            if name.endswith("-holdout"):
                report=read(directory/(name+".json"))
                reports[name]=report
                for row in report["cases"]:
                    step=row["observed"].get("step",{})
                    if row["outcome"]=="passed" and step.get("status")=="clarification":
                        expected.setdefault((name,row["id"],step["message"]),[]).append(row["repetition"])
        seen=set()
        downgrades=Counter()
        for entry in audit["entries"]:
            key=(entry["report"],entry["id"],entry["message"])
            assert key in expected and key not in seen,"unknown/duplicate semantic audit entry"
            seen.add(key)
            assert entry["repetitions"]==sorted(expected[key]),"semantic audit trial mismatch"
            assert entry["verdict"] in ["accepted","accepted_with_reservation","rejected"]
            assert entry["reason"].strip()
            if entry["verdict"]=="rejected": downgrades[entry["report"]]+=len(entry["repetitions"])
        assert seen==set(expected),"semantic audit omitted a passing clarification"
        assert {r["report"] for r in audit["results"]}==set(reports)
        for result in audit["results"]:
            report=reports[result["report"]]
            assert result["automatic_passed"]==report["passed"]
            assert result["downgraded_trials"]==downgrades[result["report"]]
            assert result["semantic_review_passed"]==report["passed"]-downgrades[result["report"]]
            assert result["total"]==report["total"]
            assert result["unexpected_proposals"]==report["unexpected_proposals"]
    return len(names)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path)
    parser.add_argument("--skip-source-check",action="store_true",help="For historic evidence only; report and case checks remain active")
    parser.add_argument("--self-test",action="store_true",help="Also prove altered scores and omitted trials are rejected")
    parser.add_argument("--require-gate",action="append",default=[],help="Require a named model/profile/suite to pass qualification; repeat as needed")
    args=parser.parse_args()
    count=verify(args.directory,not args.skip_source_check)
    results={row["name"]:row for row in read(args.directory/"manifest.json")["results"]}
    for name in args.require_gate:
        assert name in results, f"unknown qualification gate: {name}"
        assert results[name]["gate_passed"], f"model qualification gate failed: {name}"
    if args.self_test:
        with tempfile.TemporaryDirectory(prefix="edgehome-evidence-check-") as temporary:
            root=Path(temporary).resolve()
            assert root.parent==Path(tempfile.gettempdir()).resolve()
            for mutation in ["score", "omission"]:
                copy=root/mutation
                shutil.copytree(args.directory,copy)
                manifest=read(copy/"manifest.json")
                entry=manifest["results"][0]
                path=copy/(entry["name"]+".json")
                report=read(path)
                if mutation=="score":
                    report["cases"][0]["outcome"]="passed" if report["cases"][0]["outcome"]!="passed" else "incorrect_output"
                else:
                    report["cases"].pop()
                path.write_text(json.dumps(report),encoding="utf-8")
                # Recompute the hash, so rejection must come from independent
                # scoring/completeness rather than simply a changed file hash.
                entry["report_sha256"]=sha(path)
                (copy/"manifest.json").write_text(json.dumps(manifest),encoding="utf-8")
                try:
                    verify(copy,not args.skip_source_check)
                except AssertionError:
                    pass
                else:
                    raise AssertionError(f"tamper self-test failed: {mutation}")
        print("Tamper checks passed: altered scores and omitted trials were rejected after rehashing.")
    print(f"Verified {count} reports: hashes, all trial identities, independent outcomes, aggregate metrics, and quality gates.")
if __name__=="__main__":
    try: main()
    except (AssertionError,KeyError,ValueError,OSError) as error:
        print(f"Evidence verification failed: {error}",file=sys.stderr)
        raise SystemExit(1)
