"""Interactive host for the real file example; every plan requires user approval."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import threading


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    env = os.environ.copy()
    env["EDGEHOME_FILE_ROOT"] = str(args.root.resolve(strict=True))
    env["EDGEHOME_FILE_POLICY"] = "guided"
    process = subprocess.Popen(
        [str(args.binary.resolve(strict=True)), "rpc", "--config",
         str(args.config.resolve(strict=True)), "--enable-execution"],
        env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        text=True, encoding="utf-8", bufsize=1,
    )
    lines = queue.Queue()

    def reader():
        for line in process.stdout:
            lines.put(line)
        lines.put(None)

    worker = threading.Thread(target=reader, daemon=True)
    worker.start()

    def read():
        # Host wait bound; a timeout does not prove a dispatched operation stopped.
        line = lines.get(timeout=200)
        if line is None or len(line.encode()) > 256*1024:
            raise RuntimeError("agent closed or exceeded response limit")
        return json.loads(line)

    sequence = 0

    def request(**command):
        nonlocal sequence
        sequence += 1
        data = json.dumps({"id":sequence,"command":command}, ensure_ascii=False)+"\n"
        if len(data.encode()) > 8192:
            raise ValueError("request exceeds RPC frame budget")
        process.stdin.write(data)
        process.stdin.flush()
        result = read()
        if result.get("id") != sequence:
            raise RuntimeError("RPC response ID mismatch")
        if result.get("type") == "error":
            print(result["message"])
            if result.get("clarification"):
                return {"status":"clarification","clarification":result["clarification"]}
            raise RuntimeError(result["code"])
        return result["result"]

    try:
        ready = read()
        if ready.get("protocol") != 1 or "clarification_resolve" not in ready.get("capabilities",[]):
            raise RuntimeError("agent does not support resumable clarification")
        step = request(method="prompt", input=input("Task: "), tools=["file.list","file.read","file.stats"])
        while True:
            if step.get("status") == "clarification":
                question = step["clarification"]
                print(question["message"])
                if question["fields"]:
                    values = {}
                    for field in question["fields"]:
                        print(field["prompt"])
                        choices = field["choices"]
                        if choices:
                            for i, choice in enumerate(choices, 1):
                                print(f"  {i}: {json.dumps(choice, ensure_ascii=False)}")
                            index = int(input("Choice number: "))-1
                            if not 0 <= index < len(choices):
                                raise ValueError("choice out of range")
                            values[field["name"]] = choices[index]
                        else:
                            # This file app's only field is a relative path string.
                            if field["name"] != "path":
                                raise RuntimeError("unexpected field in file application")
                            values[field["name"]] = input("Relative path: ")
                    step = request(method="resolve", clarification_id=question["clarification_id"], values=values)
                else:
                    step = request(method="reply", clarification_id=question["clarification_id"], input=input("Reply: "))
            elif step.get("status") == "proposal":
                plan = step["plan"]
                print(json.dumps(plan, ensure_ascii=False, indent=2))
                if input("Execute this exact read-only plan? [yes/no] ").strip().lower() != "yes":
                    print("Plan left unexecuted.")
                    break
                step = request(method="execute", plan_id=plan["plan_id"], approved=True)
                if step.get("status") != "clarification":
                    print(json.dumps(step, ensure_ascii=False, indent=2))
                    break
            else:
                print(json.dumps(step, ensure_ascii=False, indent=2))
                break
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        worker.join(timeout=2)
        process.stdout.close()


if __name__ == "__main__":
    main()
