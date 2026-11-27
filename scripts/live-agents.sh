#!/usr/bin/env bash
# Live test of the agent workspace on this Mac: an AI agent opens windows in Chrome Agents and in a
# native app through cua-driver, and the operator must never see them or lose focus.
#
# Usage: scripts/live-agents.sh [--cases LIST] [--max-visible-ms N] [--force]
#   --cases           comma list (default all but chrome-restart):
#                       chrome-window   agent opens a new Chrome Agents window, loads a page, closes it
#                       chrome-restart  agent stops Chrome Agents, starts it again, loads a page, releases
#                                       (closes every agent's tabs: refused while a lease is in use)
#                       cua-calculator  agent launches Calculator, presses 7 + 5 =, reads 12, quits it
#                       cua-textedit    agent launches TextEdit on a new file with cua-driver launch_app,
#                                       types into it, reads it back, quits it (first-window path)
#                       open-textedit   the same, launched with open -g -j (born behind the operator)
#   --max-visible-ms  how long a test window may show on a display (default 0)
#   --force           run chrome-restart even when another agent used Chrome in the last 15 minutes
#
# Needs: Spaceman running in automatic mode with [agents] workspace, Chrome Agents, cua-driver serve.
# Do not touch the keyboard or mouse during the run (about 30 s): input counts as explicit focus.
# Results: target/live-agents/<time>/ (summary.toml, winwatch.log, agent.log, cases.tsv). Exit 1 on a fail.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
here="$root/scripts/live-agents"
SPACEMAN="${SPACEMAN:-$HOME/.local/bin/spaceman}"
SKILLS="${SKILLS:-/Users/tao/Tools/claude-skills/skills/browser-use/scripts}"
SUPPORT="$HOME/Library/Application Support/Spaceman"
cases="chrome-window,cua-calculator,cua-textedit,open-textedit"
max_visible=0
force=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --cases) cases=$2; shift 2 ;;
    --max-visible-ms) max_visible=$2; shift 2 ;;
    --force) force=1; shift ;;
    *) sed -n '2,22p' "$0" >&2; exit 2 ;;
  esac
done

now_ms() { perl -MTime::HiRes=time -e 'printf "%d", time * 1000'; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# Preflight.
status=$("$SPACEMAN" ctl status) || fail "Spaceman does not answer on control.sock"
[[ "$status" == *'"mode":"automatic"'* ]] || fail "Spaceman is not in automatic mode: $status"
agent_ws=$(sed -n '/^\[agents\]/,/^\[/s/^workspace *= *\([0-9]*\).*/\1/p' "$SUPPORT/config.toml" | head -1)
[[ -n "$agent_ws" ]] || fail "config.toml has no [agents] workspace"
cua-driver status >/dev/null 2>&1 || fail "cua-driver daemon is not running (cua-driver serve)"

run="$root/target/live-agents/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$run" "$root/target/live-agents/bin"
watcher="$root/target/live-agents/bin/winwatch"
if [[ ! -x "$watcher" || "$here/winwatch.swift" -nt "$watcher" ]]; then
  swiftc -O -o "$watcher" "$here/winwatch.swift" 2>/dev/null || fail "cannot build winwatch"
fi

log_start=$(stat -f %z "$SUPPORT/agent.log")
"$watcher" >"$run/winwatch.log" &
watch_pid=$!
trap 'kill "$watch_pid" 2>/dev/null || true' EXIT
sleep 0.5
: >"$run/cases.tsv"
record() { printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$(now_ms)" "$4" >>"$run/cases.tsv"; }

agent_use() { AGENT_NAME="spaceman-live-$$" "$SKILLS/agent-use.sh" "$@"; }
chrome_pid() { pgrep -f "^/Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir=$HOME/.agent-chrome( |\$)" | head -1; }

case_chrome_window() {
  local start pid out
  start=$(now_ms)
  "$SKILLS/agent-chrome/agent-chrome.sh" start >/dev/null 2>&1 || { record chrome-window "" "$start" "Chrome Agents is down"; return; }
  pid=$(chrome_pid)
  # What lease() does when Chrome Agents has no window: a new window, born off screen.
  out=$(agent_use <<'PY' 2>&1 | tail -1
import time
t = _raw("Target.createTarget", url="https://example.com/", newWindow=True, background=True,
         focus=False, left=OFF_SCREEN[0], top=OFF_SCREEN[1], width=1300, height=850)["targetId"]
title = ""
for _ in range(40):
    title = next((x["title"] for x in _raw("Target.getTargets")["targetInfos"] if x["targetId"] == t), "")
    if title and title != "about:blank" and "example.com/" not in title: break
    time.sleep(0.25)
time.sleep(2)  # leave the window up long enough for Spaceman to react, if it would
_raw("Target.closeTarget", targetId=t)
print("ok " + title if title == "Example Domain" else "title " + repr(title))
PY
) || true
  agent_use --release >/dev/null 2>&1 || true
  record chrome-window "$pid" "$start" "${out:-no output}"
}

case_chrome_restart() {
  local start pid out busy
  start=$(now_ms)
  busy=$(find "$HOME/.agent-chrome/leases" -maxdepth 1 -type f ! -name '.*' ! -name "spaceman-live-*" -mmin -15 -exec basename {} \; | tr '\n' ' ')
  if [[ -n "$busy" && $force -eq 0 ]]; then
    record chrome-restart "" "$start" "skipped: leases in use: $busy(--force runs it anyway)"
    return
  fi
  "$SKILLS/agent-chrome/agent-chrome.sh" stop >/dev/null 2>&1 || true
  "$SKILLS/agent-chrome/agent-chrome.sh" start --now >/dev/null 2>&1 || { record chrome-restart "" "$start" "start --now failed"; return; }
  pid=$(chrome_pid)
  out=$(agent_use <<'PY' 2>&1 | tail -1
import time
go("https://example.com/")
time.sleep(2)
t = js("(()=>document.title)()")
print("ok " + t if t == "Example Domain" else "title " + repr(t))
PY
) || true
  agent_use --release >/dev/null 2>&1 || true
  record chrome-restart "$pid" "$start" "${out:-no output}"
}

case_cua_calculator() {
  local start launch pid wid out
  start=$(now_ms)
  "$SPACEMAN" ctl agent expect com.apple.calculator >/dev/null
  launch=$(cua-driver call launch_app '{"bundle_id":"com.apple.calculator","creates_new_application_instance":true,"additional_arguments":["-ApplePersistenceIgnoreState","YES"]}' 2>&1)
  pid=$(printf '%s' "$launch" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("pid",""))' 2>/dev/null || true)
  [[ -n "$pid" ]] || { record cua-calculator "" "$start" "launch_app: ${launch:0:120}"; return; }
  "$SPACEMAN" ctl agent claim "$pid" >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    wid=$(cua-driver call list_windows "{\"pid\":$pid}" | python3 -c 'import json,sys; w=json.load(sys.stdin).get("windows",[]); print(w[0]["window_id"] if w else "")')
    [[ -n "$wid" ]] && break
    sleep 0.25
  done
  out=$(PID=$pid WID=$wid SESSION="spaceman-live-$$" python3 "$here/calculator.py" 2>&1 | tail -1) || true
  kill "$pid" 2>/dev/null || true
  "$SPACEMAN" ctl agent release "$pid" >/dev/null 2>&1 || true
  record cua-calculator "$pid" "$start" "${out:-no output}"
}

# TextEdit opens a document it never saw at its default spot on the display: the first-window path.
# $1 = case name, $2 = launcher: cua (cua-driver launch_app) or open (open -g -j: behind the operator).
case_textedit() {
  local name=$1 how=$2 start file launch pid wid out
  start=$(now_ms)
  file="$run/$name.txt"
  : >"$file"
  "$SPACEMAN" ctl agent expect com.apple.TextEdit >/dev/null
  if [[ "$how" == cua ]]; then
    launch=$(cua-driver call launch_app "{\"bundle_id\":\"com.apple.TextEdit\",\"urls\":[\"$file\"],\"creates_new_application_instance\":true,\"additional_arguments\":[\"-ApplePersistenceIgnoreState\",\"YES\"]}" 2>&1)
    pid=$(printf '%s' "$launch" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("pid",""))' 2>/dev/null || true)
  else
    open -g -j -n -a TextEdit "$file" --args -ApplePersistenceIgnoreState YES
    for _ in $(seq 1 20); do pid=$(pgrep -nx TextEdit || true); [[ -n "$pid" ]] && break; sleep 0.1; done
  fi
  [[ -n "$pid" ]] || { record "$name" "" "$start" "launch failed: ${launch:0:120}"; return; }
  "$SPACEMAN" ctl agent claim "$pid" >/dev/null 2>&1 || true
  for _ in $(seq 1 20); do
    wid=$(cua-driver call list_windows "{\"pid\":$pid}" | python3 -c 'import json,sys; w=json.load(sys.stdin).get("windows",[]); print(w[0]["window_id"] if w else "")')
    [[ -n "$wid" ]] && break
    sleep 0.25
  done
  out=$(PID=$pid WID=$wid SESSION="spaceman-live-$$" python3 "$here/textedit.py" 2>&1 | tail -1) || true
  kill "$pid" 2>/dev/null || true
  "$SPACEMAN" ctl agent release "$pid" >/dev/null 2>&1 || true
  record "$name" "$pid" "$start" "${out:-no output}"
}

IFS=',' read -ra wanted <<<"$cases"
for c in "${wanted[@]}"; do
  case "$c" in
    chrome-window) case_chrome_window ;;
    chrome-restart) case_chrome_restart ;;
    cua-calculator) case_cua_calculator ;;
    cua-textedit) case_textedit cua-textedit cua ;;
    open-textedit) case_textedit open-textedit open ;;
    *) fail "unknown case $c" ;;
  esac
  sleep 1
done

sleep 2
kill "$watch_pid" 2>/dev/null || true
wait "$watch_pid" 2>/dev/null || true
tail -c +"$((log_start + 1))" "$SUPPORT/agent.log" >"$run/agent.log"
"$SPACEMAN" ctl status >"$run/status.json"
python3 "$here/check.py" "$run" "$agent_ws" "$max_visible"
