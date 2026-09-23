#!/usr/bin/env bash
# Tidy up after cards that reached Done. Safe to run again at any time.
#
#   done-cleanup.sh [--board <id>] [--column <name>] [--dry-run]   every card in the column
#   done-cleanup.sh [--board <id>] [--dry-run] card <card-id>       one card
#
# For each card, in this order:
#   1. stops its test server: closes the "card-<id> server" tab, then stops
#      anything still listening on the card's port (REVIEW_PORT_BASE + id)
#   2. closes its agent: the "card-<id>" tab, which ends the harness
#   3. closes its Herdr workspace when the card asked for a new one
#      (space_kind new_workspace) and nothing else is left in it
#   4. removes its worktree $HOME/.herdr/worktrees/<repo>/card-<id> with a plain
#      `git worktree remove` (never --force). The branch card-<id> stays. A
#      worktree with uncommitted work is kept; the card gets one comment saying so
#      and the tag `worktree-kept`.
#
# Cards with a queued or running run are skipped. --column defaults to Done.
# Runs on a timer (launchd dev.herdr-board.done-cleanup), so a card dragged to
# Done by hand is cleaned within a minute too.
set -euo pipefail

board_bin="${BOARD_BIN:-board}"
herdr_bin="${HERDR_BIN_PATH:-herdr}"
port_base="${REVIEW_PORT_BASE:-3200}"
column="Done" dry_run=0 sel=() one=""

while [ $# -gt 0 ]; do
  case "$1" in
    --board) sel=(--board "$2"); shift 2 ;;
    --column) column="$2"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    card) one="${2:?usage: done-cleanup.sh card <card-id>}"; shift 2 ;;
    *) echo "done-cleanup: unknown argument $1" >&2; exit 2 ;;
  esac
done

b() { "$board_bin" ${sel[@]+"${sel[@]}"} "$@"; }
say() { echo "done-cleanup: card $1: $2"; }
# Runs the command, or only prints it under --dry-run.
act() { if [ "$dry_run" = 1 ]; then echo "  would run: $*"; else "$@"; fi; }

# `json <python expression over d>` reads JSON on stdin and prints the expression.
json() { python3 -c "import json,sys; d=json.load(sys.stdin); v=$1; print('' if v is None else v)"; }

# Tab ids with this exact label, one per line.
tabs_labelled() {
  "$herdr_bin" tab list | python3 -c '
import json, sys
for t in json.load(sys.stdin)["result"]["tabs"]:
    if t.get("label") == sys.argv[1]:
        print(t["tab_id"])
' "$1"
}

stop_server() {
  local card="$1" tab port pids
  for tab in $(tabs_labelled "card-$card server"); do
    say "$card" "closing server tab $tab"
    act "$herdr_bin" tab close "$tab" >/dev/null
  done
  port=$((port_base + card))
  pids="$(lsof -ti "tcp:$port" -sTCP:LISTEN 2>/dev/null || true)"
  if [ -n "$pids" ]; then
    say "$card" "stopping what still listens on port $port (pid $(echo $pids))"
    act kill $pids
  fi
}

close_agent() {
  local card="$1" tab
  for tab in $(tabs_labelled "card-$card"); do
    say "$card" "closing agent tab $tab"
    act "$herdr_bin" tab close "$tab" >/dev/null
  done
}

# The card's own workspace, only when every tab left in it belongs to the card.
close_workspace() {
  local card="$1" label="$2" ws
  [ -n "$label" ] || return 0
  ws="$("$herdr_bin" workspace list | python3 -c '
import json, sys
for w in json.load(sys.stdin)["result"]["workspaces"]:
    if w.get("label") == sys.argv[1]:
        print(w["workspace_id"]); break
' "$label")"
  [ -n "$ws" ] || return 0
  local others
  others="$("$herdr_bin" tab list --workspace "$ws" | python3 -c '
import json, sys
mine = {"card-" + sys.argv[1], "card-" + sys.argv[1] + " server"}
print(sum(1 for t in json.load(sys.stdin)["result"]["tabs"] if t.get("label") not in mine))
' "$card")"
  if [ "$others" = 0 ]; then
    say "$card" "closing workspace $label ($ws)"
    act "$herdr_bin" workspace close "$ws" >/dev/null
  else
    say "$card" "keeping workspace $label ($ws): $others other tab(s) in it"
  fi
}

remove_worktree() {
  local card="$1" cwd="$2" tags="$3" repo wt out
  [ -n "$cwd" ] && [ -d "$cwd" ] || return 0
  repo="$(dirname "$(git -C "$cwd" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)")" || return 0
  wt="$HOME/.herdr/worktrees/$(basename "$repo")/card-$card"
  [ -d "$wt" ] || return 0
  say "$card" "removing worktree $wt"
  [ "$dry_run" = 1 ] && { echo "  would run: git -C $repo worktree remove $wt"; return 0; }
  if out="$(git -C "$repo" worktree remove "$wt" 2>&1)"; then
    return 0
  fi
  say "$card" "worktree kept: $out"
  case "$tags" in *'"worktree-kept"'*) return 0 ;; esac
  b comment "$card" "Done cleanup kept the worktree $wt: $out" >/dev/null
  local keep=()
  while IFS= read -r t; do [ -n "$t" ] && keep+=(--tag "$t"); done \
    < <(python3 -c 'import json,sys; [print(t) for t in json.loads(sys.argv[1])]' "$tags")
  b card edit "$card" "${keep[@]}" --tag worktree-kept >/dev/null
}

clean() {
  local c="$1" id status kind ref cwd tags
  id="$(json 'd["id"]' <<<"$c")"
  status="$(json 'd["status"]' <<<"$c")"
  kind="$(json 'd["space_kind"]' <<<"$c")"
  ref="$(json 'd.get("space_ref")' <<<"$c")"
  cwd="$(json 'd.get("space_cwd")' <<<"$c")"
  tags="$(json 'json.dumps(d.get("tags") or [])' <<<"$c")"
  case "$status" in queued|running) say "$id" "skipped, it is $status"; return 0 ;; esac
  stop_server "$id"
  close_agent "$id"
  [ "$kind" = new_workspace ] && close_workspace "$id" "$ref"
  remove_worktree "$id" "$cwd" "$tags"
}

if [ -n "$one" ]; then
  cards="[$(b card show "$one" --json | json 'json.dumps(d["card"])')]"
else
  cards="$(b card list --column "$column" --json)"
fi

while IFS= read -r c; do
  [ -n "$c" ] && clean "$c"
done < <(python3 -c 'import json,sys; [print(json.dumps(c)) for c in json.load(sys.stdin)]' <<<"$cards")
