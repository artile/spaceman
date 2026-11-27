"""Judge a live-agents run: winwatch log + Spaceman log slice + case results -> verdicts.

Usage: check.py RUN_DIR AGENT_WORKSPACE MAX_VISIBLE_MS
RUN_DIR holds winwatch.log, agent.log (the slice written during the run) and cases.tsv
(case, pid, start_ms, end_ms, functional result). Writes summary.toml and prints a table.
Exit 1 when any check fails.
"""
import json
import re
import sys
from pathlib import Path

run, agent_ws, max_visible = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
watch = (run / "winwatch.log").read_text().splitlines()
log = (run / "agent.log").read_text()
epoch = int(next(l for l in watch if " START " in l).split()[2])
cases = [line.split("\t") for line in (run / "cases.tsv").read_text().splitlines() if line]

win = re.compile(r"^(\d+) WIN (\d+) pid=(\d+) \S+ \S+ on=\w+ visible=(\d+) z=(-?\d+)")
front = re.compile(r"^(\d+) FRONT pid=(\d+) (\S+)")
results = []


def check(case, name, ok, detail):
    results.append((case, name, ok, detail))


for case, pid, start, end, functional in cases:
    start, end = int(start) - epoch, int(end) - epoch
    pids = {int(p) for p in pid.split(",") if p}
    check(case, "works", functional.startswith("ok"), functional)

    # Time each window of the case spent visible on a display.
    shown, visible_ms, worst = {}, {}, 0
    for line in watch:
        m = win.match(line)
        if not m or int(m[3]) not in pids:
            continue
        t, wid, pct = int(m[1]), m[2], int(m[4])
        if not start <= t <= end + 2000:
            continue
        if pct > 0 and wid not in shown:
            shown[wid] = t
        elif pct == 0 and wid in shown:
            visible_ms[wid] = visible_ms.get(wid, 0) + t - shown.pop(wid)
    for wid, t in shown.items():
        visible_ms[wid] = visible_ms.get(wid, 0) + max(0, end - t)
    worst = max(visible_ms.values(), default=0)
    detail = ", ".join(f"window {w}: {v} ms" for w, v in visible_ms.items()) or "never on a display"
    check(case, "unseen", worst <= max_visible, detail)

    # Focus never moves to the case's app.
    stolen = [
        line for line in watch
        if (m := front.match(line)) and int(m[2]) in pids and start <= int(m[1]) <= end + 2000
    ]
    check(case, "focus kept", not stolen, stolen[0] if stolen else "front app never changed to it")

    # Spaceman routed its windows to the agent workspace (or never had to: off screen).
    routed = [
        line for line in log.splitlines()
        if any(f"pid: {p}," in line for p in pids) and f"workspace {agent_ws}" in line
    ]
    switched = re.findall(rf"switch: workspace \d+ -> {agent_ws} ", log)
    check(case, "agent workspace", bool(routed) or not visible_ms, f"{len(routed)} route line(s)")
    check(case, "no switch", not switched, f"{len(switched)} switch(es) to {agent_ws}")

errors = re.findall(r"^.*(?:worker error|switch rollback|restore \(restore all\)).*$", log, re.M)
check("spaceman", "no errors", not errors, errors[0][:160] if errors else "clean log")

with open(run / "summary.toml", "w") as out:
    for case, name, ok, detail in results:
        # A JSON string is a valid TOML basic string.
        out.write(f"[[check]]\ncase = {json.dumps(case)}\nname = {json.dumps(name)}\n")
        out.write(f"ok = {str(ok).lower()}\ndetail = {json.dumps(detail)}\n\n")

width = max(len(c) for c, *_ in results)
for case, name, ok, detail in results:
    print(f"{'PASS' if ok else 'FAIL'}  {case:<{width}}  {name:<15}  {detail}")
failed = sum(not ok for *_, ok, _ in results)
print(f"\n{len(results) - failed} passed, {failed} failed  ({run})")
sys.exit(1 if failed else 0)
