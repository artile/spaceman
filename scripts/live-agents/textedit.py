"""cua-driver types into a TextEdit document in the background and reads it back.

Env: PID, WID (window id), SESSION (cua-driver session name). Prints "ok <text>" or what went wrong.
"""
import json
import os
import subprocess
import time

pid, wid, session = int(os.environ["PID"]), int(os.environ["WID"]), os.environ["SESSION"]
TEXT = "agent was here"


def call(tool, **args):
    args |= {"session": session}
    out = subprocess.run(["cua-driver", "call", tool, json.dumps(args)], capture_output=True, text=True)
    return json.loads(out.stdout)


def text_area():
    elements = call("get_window_state", pid=pid, window_id=wid).get("elements", [])
    return next((e for e in elements if e.get("role") == "AXTextArea"), None)


for _ in range(20):
    area = text_area()
    if area:
        break
    time.sleep(0.25)
else:
    raise SystemExit("no text area")
result = call("type_text", pid=pid, window_id=wid, element_token=area["element_token"], text=TEXT)
if result.get("refusal"):
    raise SystemExit(f"type_text: {result['refusal']['code']}")
value = (text_area() or {}).get("value", "")
print(f"ok {value}" if TEXT in value else f"value {value!r}")
