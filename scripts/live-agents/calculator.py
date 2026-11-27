"""cua-driver drives Calculator in the background: clear, 7 + 5 =, read the display.

Env: PID, WID (window id), SESSION (cua-driver session name). Prints "ok 12" or what went wrong.
"""
import json
import os
import re
import subprocess
import time

pid, wid, session = int(os.environ["PID"]), int(os.environ["WID"]), os.environ["SESSION"]


def call(tool, **args):
    args |= {"session": session}
    out = subprocess.run(["cua-driver", "call", tool, json.dumps(args)], capture_output=True, text=True)
    return json.loads(out.stdout)


def state():
    return call("get_window_state", pid=pid, window_id=wid)


def press(label):
    for element in state()["elements"]:
        if element.get("role") == "AXButton" and element.get("label") == label:
            result = call("click", pid=pid, window_id=wid, element_token=element["element_token"])
            if result.get("refusal"):
                raise SystemExit(f"click {label}: {result['refusal']['code']}")
            return
    raise SystemExit(f"no button {label!r}")


def first(*names):
    return next((n for n in names if n in labels), None)


def display():
    # "12" in basic mode; "C, subscript 16" in programmer mode with base 16.
    tree = state()["tree_markdown"]
    match = re.search(r'AXStaticText = "([^"]*)" \(Edit field\)', tree) or re.search(
        r'AXStaticText = "([^"]*)"', tree
    )
    return match[1] if match else ""


# Accessibility gives no elements for a window on another macOS Space: the operator works in a
# native fullscreen app, and new windows open on the desktop Space behind it.
listed = call("list_windows", pid=pid)
window = next((w for w in listed.get("windows", []) if w["window_id"] == wid), {})
if window and not window.get("on_current_space", True):
    raise SystemExit(
        f"env: window is on Space {window.get('space_ids')}, operator on Space "
        f"{listed.get('current_space_id')} (native fullscreen?); cua-driver cannot read it"
    )
# A fresh launch can answer before its buttons are in the Accessibility tree.
for _ in range(20):
    labels = {e.get("label") for e in state()["elements"] if e.get("role") == "AXButton"}
    if "Equals" in labels:
        break
    time.sleep(0.25)
else:
    raise SystemExit(f"no keypad: {sorted(filter(None, labels))[:12]}")
clear = first("All Clear", "Clear")
if clear:
    press(clear)
for label in ("7", "Add", "5", "Equals"):
    press(label)
shown = display().replace("‎", "").replace(",", "", 0)
base = re.search(r"subscript (\d+)", shown)
digits = shown.split(",")[0].strip()
try:
    value = int(digits, int(base[1]) if base else 10)
except ValueError:
    raise SystemExit(f"display {shown!r}")
print(f"ok {value}" if value == 12 else f"display {shown!r} = {value}")
