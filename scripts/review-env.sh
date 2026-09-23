#!/usr/bin/env bash
# Get a card's work ready for a human to test: its worktree has env files,
# dependencies, a fresh generated client, every migration applied, and a dev
# server running in a Herdr tab of its own. Safe to run again at any time.
#
#   review-env.sh ensure <card-id>   start the server if it is not answering; print its URL
#   review-env.sh open               the card selected in the board TUI: open its agent
#                                    tab and the browser at its server (bind to a key)
#   review-env.sh stop <card-id>     close the card's server tab
#   review-env.sh serve <card-id>    (internal) the setup + dev server the server tab runs
#
# The worktree is the pipeline's: $HOME/.herdr/worktrees/<repo>/card-<id>, branch
# card-<id>. The server listens on port REVIEW_PORT_BASE + card id
# (REVIEW_PORT_BASE defaults to 3200, so card 4 -> http://localhost:3204).
#
# Setup steps run only when the project has what they need:
#   - gitignored .env* files are copied fresh from the main checkout, with the main
#     dev port (from package.json's "dev" script) rewritten to the card's port, so
#     NEXTAUTH_URL and similar point at this server
#   - package.json: `npm ci` when node_modules is missing, a symlink (Turbopack
#     refuses a node_modules that points outside the project), or older than
#     package-lock.json
#   - prisma/schema.prisma: `npx prisma generate` then `npx prisma migrate deploy`
#     (migrations go to the database the .env names, the same one the main
#     checkout uses)
set -euo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"
board_bin="${BOARD_BIN:-board}"
port_base="${REVIEW_PORT_BASE:-3200}"
# How long `ensure` waits for the server to answer, in seconds (npm ci + first
# compile can take a few minutes).
ready_timeout="${REVIEW_READY_TIMEOUT:-600}"
selection_file="${BOARD_TUI_SELECTION:-$HOME/.local/share/herdr-board/tui-selection.json}"
self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

die() { echo "review-env: $*" >&2; exit 1; }

# `json <python expression over d>` reads JSON on stdin and prints the expression.
json() { python3 -c "import json,sys; d=json.load(sys.stdin); v=$1; print('' if v is None else v)"; }

card_json() { "$board_bin" card show "$1" --json; }

# The main checkout of the card's repo, from the card's working directory.
repo_of() {
  local cwd
  cwd="$(card_json "$1" | json 'd["card"].get("space_cwd")')"
  [ -n "$cwd" ] || die "card $1 has no working directory"
  dirname "$(git -C "$cwd" rev-parse --path-format=absolute --git-common-dir)"
}

worktree_of() { echo "$HOME/.herdr/worktrees/$(basename "$(repo_of "$1")")/card-$1"; }
port_of() { echo $((port_base + $1)); }
url_of() { echo "http://localhost:$(port_of "$1")"; }
server_label() { echo "card-$1 server"; }

answering() { curl -s -o /dev/null --max-time 3 "$1"; }

# The tab id of the card's server tab, empty when there is none.
server_tab() {
  "$herdr_bin" tab list | python3 -c '
import json, sys
label = sys.argv[1]
for t in json.load(sys.stdin)["result"]["tabs"]:
    if t.get("label") == label:
        print(t["tab_id"]); break
' "$(server_label "$1")"
}

serve() {
  local card="$1" wt repo port main_port
  repo="$(repo_of "$card")"
  wt="$(worktree_of "$card")"
  port="$(port_of "$card")"
  [ -d "$wt" ] || die "no worktree at $wt (the card's branch is card-$card)"
  cd "$wt"

  main_port="$(python3 -c '
import json, re, sys
script = json.load(open(sys.argv[1])).get("scripts", {}).get("dev", "")
m = re.search(r"(?:--port|-p)[ =](\d+)", script)
print(m.group(1) if m else "3000")
' "$repo/package.json" 2>/dev/null || echo 3000)"

  echo "== env files from $repo (localhost:$main_port -> localhost:$port)"
  local f name
  for f in "$repo"/.env*; do
    [ -f "$f" ] || continue
    name="$(basename "$f")"
    git -C "$repo" check-ignore -q "$name" || continue   # only the untracked, local ones
    sed "s#localhost:$main_port#localhost:$port#g; s#127.0.0.1:$main_port#127.0.0.1:$port#g" "$f" > "$wt/$name"
    echo "   $name"
  done

  if [ -f package.json ]; then
    if [ -L node_modules ] || [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
      echo "== npm ci"
      [ -L node_modules ] && rm node_modules
      npm ci
    fi
  fi

  if [ -f prisma/schema.prisma ]; then
    echo "== prisma generate"
    npx prisma generate
    echo "== prisma migrate deploy"
    npx prisma migrate deploy
  fi

  echo "== dev server on $(url_of "$card")"
  export PORT="$port"
  if [ -x node_modules/.bin/next ]; then
    exec npx next dev --port "$port"
  fi
  exec npm run dev
}

ensure() {
  local card="$1" url tab out pane waited=0
  url="$(url_of "$card")"
  if answering "$url"; then echo "$url"; return 0; fi

  [ -d "$(worktree_of "$card")" ] || die "no worktree at $(worktree_of "$card")"
  # A server tab whose server stopped: start over in a fresh tab.
  tab="$(server_tab "$card")"
  [ -n "$tab" ] && "$herdr_bin" tab close "$tab" >/dev/null

  local workspace
  workspace="$(card_json "$card" | json 'd["card"].get("space_ref") if d["card"].get("space_kind") == "workspace" else None')"
  out="$("$herdr_bin" tab create ${workspace:+--workspace "$workspace"} \
    --cwd "$(worktree_of "$card")" --label "$(server_label "$card")" --no-focus)"
  pane="$(printf '%s' "$out" | json 'd["result"]["root_pane"]["pane_id"]')"
  "$herdr_bin" pane run "$pane" "bash '$self' serve $card" >/dev/null

  echo "review-env: card $card: setting up in tab '$(server_label "$card")', waiting for $url" >&2
  until answering "$url"; do
    if [ "$waited" -ge "$ready_timeout" ]; then
      die "card $card: no answer from $url after ${ready_timeout}s; see tab '$(server_label "$card")'"
    fi
    sleep 3; waited=$((waited + 3))
  done
  echo "$url"
}

stop() {
  local tab
  tab="$(server_tab "$1")"
  if [ -n "$tab" ]; then "$herdr_bin" tab close "$tab" >/dev/null; echo "closed $(server_label "$1")"; fi
}

# Bring the card's agent to the front: its newest run's pane (the board reopens
# the session when the pane is gone), else the card's `card-<id>` tab.
focus_agent() {
  local card="$1" board="$2" run
  run="$(card_json "$card" | json 'max((r["id"] for r in d["runs"]), default=None)')"
  if [ -n "$run" ] && [ -n "${HERDR_SOCKET_PATH:-}" ] &&
    "$board_bin" --board "$board" card run focus "$card" "$run" --origin-socket "$HERDR_SOCKET_PATH" >/dev/null 2>&1; then
    return 0
  fi
  local tab
  tab="$("$herdr_bin" tab list | python3 -c '
import json, sys
for t in json.load(sys.stdin)["result"]["tabs"]:
    if t.get("label") == sys.argv[1]:
        print(t["tab_id"]); break
' "card-$card")"
  [ -n "$tab" ] && "$herdr_bin" tab focus "$tab" >/dev/null
}

open_selected() {
  [ -f "$selection_file" ] || die "no card selected yet (open the board and pick a card)"
  local card board url
  card="$(json 'd.get("card_id")' < "$selection_file")"
  board="$(json 'd.get("board_id")' < "$selection_file")"
  [ -n "$card" ] || die "no card selected in the board"

  focus_agent "$card" "$board" || true
  # The server may need minutes to set up; the browser opens once it answers.
  local log="${selection_file%/*}/review-env.log"
  mkdir -p "${log%/*}"
  (
    if url="$(ensure "$card" 2>>"$log")"; then
      if command -v open >/dev/null; then open "$url"; else xdg-open "$url"; fi
    else
      "$herdr_bin" notification show "card $card" --body "review server did not start; see tab '$(server_label "$card")'" >/dev/null 2>&1 || true
    fi
  ) </dev/null >/dev/null 2>&1 &
  disown
}

case "${1:-}" in
  ensure) [ -n "${2:-}" ] || die "usage: review-env.sh ensure <card-id>"; ensure "$2" ;;
  serve) [ -n "${2:-}" ] || die "usage: review-env.sh serve <card-id>"; serve "$2" ;;
  stop) [ -n "${2:-}" ] || die "usage: review-env.sh stop <card-id>"; stop "$2" ;;
  open) open_selected ;;
  *) die "usage: review-env.sh ensure|serve|stop <card-id> | open" ;;
esac
