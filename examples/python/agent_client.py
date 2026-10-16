"""Standard-library stdio client. Only approves the immutable starter fixture."""
import json
import subprocess
import sys


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: python client.py PATH_TO_AGENT_BINARY")
    with subprocess.Popen(
        [sys.argv[1], "rpc", "--mock", "--enable-execution"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        text=True, encoding="utf-8", bufsize=1,
    ) as process:
        try:
            ready = json.loads(process.stdout.readline())
            if ready.get("protocol") != 1:
                raise RuntimeError("unsupported protocol")

            def request(request_id, **command):
                process.stdin.write(json.dumps({"id": request_id, "command": command}) + "\n")
                process.stdin.flush()
                response = json.loads(process.stdout.readline())
                if response.get("id") != request_id or response.get("type") != "result":
                    raise RuntimeError(response)
                return response["result"]

            step = request(1, method="prompt", input="Count the sample document", tools=["document.stats"])
            if step.get("status") != "proposal":
                raise RuntimeError("expected a proposal")
            plan = step["plan"]
            # Never replace this with blanket approval for arbitrary tools.
            if plan["tool"] != "document.stats" or plan["arguments"] != {"document": "sample"}:
                raise RuntimeError("unexpected tool proposal")
            receipt = request(2, method="execute", plan_id=plan["plan_id"], approved=True)
            if receipt["status"] != "succeeded" or receipt["output"] != {"characters": 84, "lines": 2}:
                raise RuntimeError("unexpected execution result")
            print(json.dumps(receipt, ensure_ascii=False))
            process.stdin.close()
            process.wait(timeout=5)
            if process.returncode:
                raise RuntimeError("agent exited unsuccessfully")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
