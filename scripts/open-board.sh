#!/usr/bin/env bash
# Idempotent launcher for the board overlay — used by both the `open-board` action
# and a herdr keybinding (`[[keys.command]]` with `type = "shell"`). Mirrors
# herdr-file-viewer's launcher: "open-or-focus, toggle off on repeat".
#
#   - no board pane in the current workspace     -> open it (focused; a tab is
#                                                     brought to the front too)
#   - a board pane exists but isn't focused        -> focus it (and its tab)
#   - the focused pane IS the board pane           -> overlay: close it (herdr has no
#                                                     hide-without-close; reopening is
#                                                     cheap — the TUI refetches);
#                                                     tab/split/zoomed: keep it focused
#
# herdr actions/keybindings run a command (no declarative "open this pane" field),
# so this shells out to the herdr CLI via $HERDR_BIN_PATH (herdr injects it; fall
# back to `herdr` on PATH). The pane is identified by its static title (`Board`),
# the legacy filter title, or the scoped title (`Board [scope · FILTER]`). Any failure
# degrades to OPEN, preserving always-open behavior.
set -uo pipefail

herdr_bin="${HERDR_BIN_PATH:-herdr}"

# HERDR_BOARD_PLACEMENT picks where the board opens: overlay (default), tab, split, zoomed.
# The pane does not inherit this shell's environment, so the editor settings the TUI's
# Ctrl+E needs ($EDITOR and what nvim reads its config through) are passed along explicitly.
placement="${HERDR_BOARD_PLACEMENT:-overlay}"

# A shortcut run by the Herdr server often has no $EDITOR: the server was not
# started from a shell that set it (zsh users set it in ~/.zshenv, which bash
# never reads). Ask the user's own login shell before falling back to the
# TUI's default (vi).
if [ -z "${EDITOR:-}" ] && [ -z "${VISUAL:-}" ]; then
  EDITOR="$("${SHELL:-/bin/zsh}" -lc 'printf %s "${EDITOR:-$VISUAL}"' 2>/dev/null </dev/null | tail -n 1)"
  [ -n "$EDITOR" ] && export EDITOR || unset EDITOR
fi

open_pane() {
  local env_args=()
  local name
  for name in EDITOR VISUAL NVIM_APPNAME XDG_CONFIG_HOME XDG_DATA_HOME COLORTERM; do
    if [ -n "${!name:-}" ]; then env_args+=(--env "$name=${!name}"); fi
  done
  "$herdr_bin" plugin pane open \
    --plugin herdr-board \
    --entrypoint board \
    --placement "$placement" \
    ${env_args[@]+"${env_args[@]}"} \
    --focus || exit $?
  # --focus focuses the pane; a new tab still needs to be brought to the front.
  if [ "$placement" != "overlay" ]; then
    local tab
    tab="$(board_tab_id)"
    [ -n "$tab" ] && "$herdr_bin" tab focus "$tab" >/dev/null 2>&1
  fi
  exit 0
}

# The tab that holds the board pane, from the live pane list (empty when unknown).
board_tab_id() {
  command -v python3 >/dev/null 2>&1 || return 0
  "$herdr_bin" pane list 2>/dev/null | python3 -c '
import json, re, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
res = data.get("result", data)
for p in (res.get("panes", []) if isinstance(res, dict) else []):
    name = (p.get("label") or p.get("title") or "")
    if name == "Board" or re.fullmatch(r"Board \[(?:(?:ACTIVE|ALL|ARCHIVED)|.+ · (?:ACTIVE|ALL|ARCHIVED))\]", name):
        print(p.get("tab_id") or ""); break
' 2>/dev/null
}

# Decide OPEN / "FOCUS <pane>" / "CLOSE <pane>" from the live pane list. Needs
# python3 for robust JSON parsing; without it we always OPEN.
decision="OPEN"
if command -v python3 >/dev/null 2>&1; then
  panes="$("$herdr_bin" pane list 2>/dev/null || true)"   # outputs JSON (no --json flag)
  if [ -n "$panes" ]; then
    decision="$(printf '%s' "$panes" | python3 -c '
import json, re, sys
try:
    data = json.load(sys.stdin)
except Exception:
    print("OPEN"); sys.exit(0)
res = data.get("result", data)
panes = res.get("panes", []) if isinstance(res, dict) else []
board = None
for p in panes:
    # The TUI appends its active archive filter via `pane rename`.
    name = (p.get("label") or p.get("title") or "")
    if name == "Board" or re.fullmatch(
        r"Board \[(?:(?:ACTIVE|ALL|ARCHIVED)|.+ · (?:ACTIVE|ALL|ARCHIVED))\]", name
    ):
        board = p
        break
if not board:
    print("OPEN"); sys.exit(0)
pid = board.get("pane_id") or ""
if not pid:
    print("OPEN"); sys.exit(0)
tab = board.get("tab_id") or "-"
if board.get("focused"):
    print("CLOSE " + str(pid) + " " + str(tab))
else:
    print("FOCUS " + str(pid) + " " + str(tab))
' 2>/dev/null || echo OPEN)"
  fi
fi

# With a tab placement, a repeat press goes back to the tab you came from, so
# one key both shows and hides the board (herdr has no "previous tab"
# command). The tab is remembered here whenever the board is shown.
return_tab_file="${TMPDIR:-/tmp}/herdr-board-return-tab"

# The tab holding the focused pane (empty when unknown).
focused_tab_id() {
  command -v python3 >/dev/null 2>&1 || return 0
  "$herdr_bin" pane list 2>/dev/null | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
res = data.get("result", data)
for p in (res.get("panes", []) if isinstance(res, dict) else []):
    if p.get("focused"):
        print(p.get("tab_id") or ""); break
' 2>/dev/null
}

remember_return_tab() {
  local tab
  tab="$(focused_tab_id)"
  [ -n "$tab" ] && printf '%s' "$tab" >"$return_tab_file" 2>/dev/null
  return 0
}

focus_pane() {  # focus_pane <pane_id> <tab_id|->
  if [ "$2" != "-" ]; then "$herdr_bin" tab focus "$2" >/dev/null 2>&1; fi
  exec "$herdr_bin" plugin pane focus "$1"
}

# A repeat press closes an overlay. A tab goes back to the tab you came from;
# a split or zoomed board just stays focused.
if [ "$placement" = "tab" ] && [ "${decision%% *}" = "CLOSE" ]; then
  board_tab="${decision##* }"
  back="$(cat "$return_tab_file" 2>/dev/null || true)"
  if [ -n "$back" ] && [ "$back" != "$board_tab" ] && "$herdr_bin" tab focus "$back" >/dev/null 2>&1; then
    exit 0
  fi
fi
if [ "$placement" != "overlay" ]; then decision="${decision/#CLOSE /FOCUS }"; fi
[ "${decision%% *}" = "CLOSE" ] || remember_return_tab

case "$decision" in
  "FOCUS "*)
    rest="${decision#FOCUS }"
    focus_pane "${rest%% *}" "${rest#* }"
    ;;
  "CLOSE "*)
    rest="${decision#CLOSE }"
    exec "$herdr_bin" pane close "${rest%% *}"
    ;;
  *)
    open_pane
    ;;
esac
