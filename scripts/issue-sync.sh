#!/usr/bin/env bash
# Keep the board's Todo lane in step with GitHub issues. Safe to run again.
#
#   issue-sync.sh --repo <owner/name> --cwd <repo checkout> [--board <id>]
#                 [--label <label>] [--column <name>]
#
# Adds a card for every open issue. An issue that already has a card, archived
# ones included, is not added again. Cards are matched by their title, which
# starts with the issue number: "#123 <issue title>".
#
# Tags every issue card so the board's `t` key can filter Todo:
#   github           always (marks the card as made from an issue)
#   ready-for-agent  the issue carries --label (default `ready-for-agent`)
#   mine             the issue is assigned to you (the `gh` login)
# Tags are brought up to date on every run, for cards in any column.
#
# Removes a card from --column (default Todo) when its issue is closed:
# deleted when it never ran, archived when it has run history. Cards past Todo
# are never removed.
#
# New cards run with the claude harness in a new Herdr workspace "issue-<n>"
# opened at --cwd; the columns choose model and effort. Closing is left to
# GitHub: the Release stage writes "Closes #<n>" in the pull request, so merging
# it closes the issue.
set -euo pipefail

board_bin="${BOARD_BIN:-board}"
herdr_bin="${HERDR_BIN_PATH:-herdr}"
repo="" cwd="" label="ready-for-agent" column="Todo" sel=()

while [ $# -gt 0 ]; do
  case "$1" in
    --repo) repo="$2"; shift 2 ;;
    --cwd) cwd="$2"; shift 2 ;;
    --board) sel=(--board "$2"); shift 2 ;;
    --label) label="$2"; shift 2 ;;
    --column) column="$2"; shift 2 ;;
    *) echo "issue-sync: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ -n "$repo" ] && [ -n "$cwd" ] || { echo "usage: issue-sync.sh --repo <owner/name> --cwd <path> [--board <id>] [--label <label>] [--column <name>]" >&2; exit 2; }

notify() { "$herdr_bin" notification show "issue sync" --body "$1" >/dev/null 2>&1 || true; }
fail() { echo "issue-sync: $1" >&2; notify "failed: $1"; exit 1; }
b() { "$board_bin" ${sel[@]+"${sel[@]}"} "$@"; }

me="$(gh api user -q .login)" || fail "gh is not signed in"
issues="$(gh issue list --repo "$repo" --state open --limit 1000 \
  --json number,title,body,url,assignees,labels)" || fail "gh issue list failed for $repo"
cards="$(b card list --visibility all --json)" || fail "board card list failed"
columns="$(b column list --json)" || fail "board column list failed"

# The plan, one JSON array per line: ["add", number, title, description] or
# ["remove", card_id, number, assignees]. Issues that have a Todo card but are
# no longer in the labelled list are looked up one by one.
# The plan, one JSON array per line: ["add", number, title, description, tags],
# ["tag", card_id, number, tags] or ["remove", card_id, number]. A Todo card
# whose issue is missing from the open list is checked one by one, so only a
# closed issue removes its card.
plan="$(python3 - "$issues" "$cards" "$columns" "$column" "$me" "$repo" "$label" <<'PY'
import json, re, subprocess, sys
issues, cards, columns = (json.loads(a) for a in sys.argv[1:4])
column, me, repo, label = sys.argv[4:8]

def number_of(card):
    m = re.match(r"#(\d+)\b", card.get("title") or "")
    return int(m.group(1)) if m else None

def tags_of(issue):
    tags = {"github"}
    if label in {l["name"] for l in issue.get("labels") or []}:
        tags.add("ready-for-agent")
    if me in {a["login"] for a in issue.get("assignees") or []}:
        tags.add("mine")
    return sorted(tags)

todo = next((c["id"] for c in columns if c["name"].lower() == column.lower()), None)
if todo is None:
    sys.exit(f"no column named {column}")

by_number = {i["number"]: i for i in issues}
have = {n for n in map(number_of, cards) if n is not None}

for i in sorted(issues, key=lambda i: i["number"]):
    if i["number"] in have:
        continue
    desc = (f"GitHub issue #{i['number']}: {i['url']}\n"
            f"(Read its comments with `gh issue view {i['number']} --comments`.)\n\n{i['body'] or ''}")
    print(json.dumps(["add", i["number"], f"#{i['number']} {i['title']}", desc, tags_of(i)]))

for card in cards:
    n = number_of(card)
    if n is None or card.get("archived_at"):
        continue
    issue = by_number.get(n)
    if issue is not None:
        tags = tags_of(issue)
        if sorted(card.get("tags") or []) != tags:
            print(json.dumps(["tag", card["id"], n, tags]))
        continue
    if card["column_id"] != todo:
        continue
    out = subprocess.run(["gh", "issue", "view", str(n), "--repo", repo, "--json", "state"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(f"issue-sync: could not read issue #{n}: {out.stderr.strip()}", file=sys.stderr)
        continue
    if json.loads(out.stdout)["state"] == "CLOSED":
        print(json.dumps(["remove", card["id"], n]))
PY
)" || fail "could not work out what to sync"

# Field $2 of the JSON array $1: strings and numbers as they are, lists as JSON.
field() { python3 -c 'import json,sys; v = json.loads(sys.argv[1])[int(sys.argv[2])]; print(v if isinstance(v, (str, int)) else json.dumps(v))' "$1" "$2"; }

# Each tag in the JSON list at $1 becomes one `--tag <tag>` argument.
tag_args() { python3 -c 'import json,sys; [print(f"--tag\n{t}") for t in json.loads(sys.argv[1])]' "$1"; }

added=0 tagged=0 removed=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  case "$(field "$line" 0)" in
    add)
      number="$(field "$line" 1)"
      tags=(); while IFS= read -r a; do tags+=("$a"); done < <(tag_args "$(field "$line" 4)")
      b card create --title "$(field "$line" 2)" --description "$(field "$line" 3)" \
        --column "$column" --harness claude --space-kind new-workspace --space-ref "issue-$number" \
        --space-cwd "$cwd" ${tags[@]+"${tags[@]}"} >/dev/null || fail "could not create the card for issue #$number"
      echo "added #$number"
      added=$((added + 1))
      ;;
    tag)
      card="$(field "$line" 1)" number="$(field "$line" 2)"
      tags=(); while IFS= read -r a; do tags+=("$a"); done < <(tag_args "$(field "$line" 3)")
      b card edit "$card" ${tags[@]+"${tags[@]}"} >/dev/null || fail "could not tag card $card (issue #$number)"
      echo "tagged card $card (issue #$number): $(field "$line" 3)"
      tagged=$((tagged + 1))
      ;;
    remove)
      card="$(field "$line" 1)" number="$(field "$line" 2)"
      runs="$(b card show "$card" --json | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["runs"]))')"
      if [ "$runs" = 0 ]; then
        b card delete "$card" --yes >/dev/null || fail "could not delete card $card (issue #$number)"
        echo "deleted card $card: issue #$number is closed"
      else
        b card archive "$card" >/dev/null || fail "could not archive card $card (issue #$number)"
        echo "archived card $card: issue #$number is closed"
      fi
      removed=$((removed + 1))
      ;;
  esac
done <<<"$plan"

echo "issue-sync: $added added, $tagged retagged, $removed removed"
# Quiet when nothing was added or removed: this also runs on a timer.
if [ $((added + removed)) -gt 0 ]; then notify "$added added, $removed removed"; fi
