//! The `pipeline` board template: the column set `template.apply` puts on an
//! empty board. Shared by the daemon and the fake client so the two cannot
//! drift.
//!
//! Todo → Spec (optional) → Plan → Execute → Review → Human Review → Release →
//! Done. Every agent stage works in its own git worktree
//! (`~/.herdr/worktrees/<repo>/card-<id>`, branch `card-<id>`), so cards can run
//! side by side. Nothing here names a project: the default branch comes from
//! `origin/HEAD` and the GitHub repo from the checkout.

use std::path::Path;

use crate::db::{ColumnTarget, ColumnWiring};
use crate::protocol::{ColumnCreateParams, Trigger};

/// The template's name on the wire (`template.apply {name}`).
pub const PIPELINE: &str = "pipeline";

/// Shared first block of every agent stage: find the repo and its default
/// branch, then create (or reuse) the card's worktree.
const WORKTREE: &str = r##"WORKTREE — do this first, every run. It is safe to run again:
  REPO=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
  BASE=$(git -C "$REPO" symbolic-ref --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')
  BASE=${BASE:-main}
  WT="$HOME/.herdr/worktrees/$(basename "$REPO")/card-$BOARD_CARD_ID"
  BR="card-$BOARD_CARD_ID"
  if [ ! -d "$WT" ]; then
    git -C "$REPO" fetch origin "$BASE"
    if git -C "$REPO" show-ref --verify --quiet "refs/heads/$BR"; then
      git -C "$REPO" worktree add "$WT" "$BR"
    else
      git -C "$REPO" worktree add "$WT" -b "$BR" "origin/$BASE"
    fi
  fi
Shell variables do not survive between commands: set REPO, BASE, WT and BR again when
you need them. Do ALL reads, edits, and commands inside $WT (cd into it in every shell
command, use absolute paths under it). Never edit files in $REPO itself."##;

/// A card whose title starts with `#<n> ` is GitHub issue `<n>` of the repo the
/// checkout points at. `verb` is what the stage does ("plan", "grill").
fn github_issue(verb: &str) -> String {
    format!(
        r##"GITHUB ISSUE — when the card title starts with "#<n> " the card is GitHub issue <n>
of this repository (run gh inside $WT, it finds the repo from the checkout):
  gh issue view <n> --json assignees -q '[.assignees[].login] | join(",")'
If it is assigned to someone and not to you (gh api user -q .login), stop: do not {verb}, run
  board comment $BOARD_CARD_ID "Issue #<n> is assigned to <them>; not taking it."
  board done $BOARD_CARD_ID --outcome fail
Otherwise take it before you {verb}:
  gh issue edit <n> --add-assignee @me"##
    )
}

const SPEC: &str = r##"You are in the SPEC stage. The human is here, in this pane, and will answer you.
Run a grill-me session with them about this card: read and follow
$HOME/.agents/skills/grill-me/SKILL.md (it points to the grilling skill next to it). If
that skill is not installed, interview them yourself the same way: relentless, one
question at a time. Read the card, its comments, the GitHub issue if there is one, and
the code in $WT before you ask. Ask ONE question at a time and wait for the answer. Do
not write code.

The goal is a spec that the next stage (Plan) can turn into an implementation plan without
asking the human again. Dig into: the problem and who has it, what done looks like
(acceptance criteria), scope and non-goals, business rules and edge cases, data and
migration impact, UI behaviour, errors, permissions, and open risks. If the repository
keeps a glossary or decision records (for example CONTEXT.md, UBIQUITOUS_LANGUAGE.md,
docs/adr/), use its words and respect its decisions.

When every branch is settled, or the human says we are done, write the spec to
  $WT/docs/specs/card-$BOARD_CARD_ID-spec.md
Sections: Problem, Goal, Acceptance criteria, Scope, Non-goals, Decisions (each with its
reason), Business rules and edge cases, Open questions (only ones the human chose to leave
open). Show the human a short summary and ask "Hand this to Plan?". On yes, run:
  board comment $BOARD_CARD_ID "Spec ready at <filepath> (worktree $WT). <3-line summary>"
  board done $BOARD_CARD_ID --outcome ok
If the human stops the session or says the card is not worth doing, run:
  board comment $BOARD_CARD_ID "<why the spec stopped>"
  board done $BOARD_CARD_ID --outcome fail"##;

const PLAN: &str = r##"You are in the PLAN stage. If a card comment says "Spec ready at <path>", read that spec
first: it holds the requirements the human already agreed to. Build the plan on it and do not
re-open decisions it settles. Use /quick-planner style planning: produce a written
implementation plan and save it under docs/plans/ (or .plans/) inside $WT. Do not write code.
When finished you MUST run:
  board comment $BOARD_CARD_ID "Plan ready at <filepath> (worktree $WT). <3-line summary>"
  board done $BOARD_CARD_ID --outcome ok"##;

const EXECUTE: &str = r##"You are in the EXECUTE stage. Implement the plan referenced in the card comments, inside $WT.
Run tests. Commit your work on branch $BR. When finished:
  board comment $BOARD_CARD_ID "<what changed, files touched, test results, branch $BR>"
  board done $BOARD_CARD_ID --outcome ok    # or --outcome fail with reasons"##;

fn review(scripts_dir: Option<&str>) -> String {
    let head = r##"You are in the REVIEW stage. Review the diff of branch $BR against origin/$BASE, the card
description, and the plan/execution comments. Be adversarial.
If the verdict is FAIL: leave the worktree in place, then
  board comment $BOARD_CARD_ID "<verdict + findings>"
  board done $BOARD_CARD_ID --outcome fail
If the verdict is OK: make sure everything is committed on $BR (git -C "$WT" status is clean).
Keep the worktree: a human tests from it next."##;
    match scripts_dir {
        Some(dir) => format!(
            r##"{head} Get it ready for them (env files, dependencies,
generated client, every migration applied, a dev server in its own tab). It can take a few minutes; give the command a 10-minute timeout:
  URL=$(bash {dir}/review-env.sh ensure $BOARD_CARD_ID)
Then:
  board comment $BOARD_CARD_ID "<verdict + findings>. Ready to test at $URL (branch $BR, worktree $WT)."
  board done $BOARD_CARD_ID --outcome ok
If the script fails, the verdict is still OK: say in the comment that the test server did not
start and why (read the tab "card-$BOARD_CARD_ID server"), then finish with --outcome ok."##
        ),
        None => format!(
            r##"{head} Then:
  board comment $BOARD_CARD_ID "<verdict + findings>. Ready to test (branch $BR, worktree $WT)."
  board done $BOARD_CARD_ID --outcome ok"##
        ),
    }
}

fn release(scripts_dir: Option<&str>) -> String {
    let stop_server = match scripts_dir {
        Some(dir) => format!(
            "0. Human review is over: stop the card's test server first, so it does not keep running:\n   bash {dir}/review-env.sh stop $BOARD_CARD_ID\n\n"
        ),
        None => String::new(),
    };
    format!(
        r##"You are in the RELEASE stage. Get branch $BR onto GitHub as a pull request that is
ready for a human to merge. Never merge it yourself and never enable auto-merge.

{stop_server}1. Confirm there is work: `git -C "$WT" log --oneline origin/$BASE..$BR` must list commits.
   If it is empty, `board done $BOARD_CARD_ID --outcome fail --summary "nothing to release"`.
2. Bring it up to date: `git -C "$WT" fetch origin "$BASE"`. If origin/$BASE moved ahead,
   `git -C "$WT" merge "origin/$BASE"` (a merge commit, never a rebase, never --force).
   On conflicts: `git merge --abort`, then fail and list the conflicting files.
3. A pre-push hook may run the project's tests. Install dependencies first if they are
   missing (with the project's own package manager, e.g. `npm ci`). Never use --no-verify.
   If the hook fails on something your branch broke, fix it, commit, push again; anything
   else is a fail.
4. Push: `git -C "$WT" push -u origin $BR`.
5. Pull request: if `gh pr view $BR` finds none, run
   `gh pr create --base "$BASE" --head $BR --title "<conventional-commit style, from the card title>" --body "<what changed and why, from the card and its comments; a Test plan section listing what was run; the card id>"`.
   If one exists, only refresh its body with `gh pr edit` when it is out of date.
   GitHub issue cards: when the card title starts with "#<n> ", leave that prefix out of the
   PR title and put the line "Closes #<n>" in the PR body, so merging closes the issue.
6. Ready to merge: `gh pr checks $BR --watch` (wait up to 30 minutes), then
   `gh pr view $BR --json url,mergeable,mergeStateStatus,reviewDecision`.
   Ready means every check passed and mergeable is MERGEABLE. If a check fails, read
   it (`gh run view <run-id> --log-failed`), fix only failures your branch caused,
   commit, push, and watch again. Anything else is a fail.
7. Finish: remove the worktree (the branch stays; never --force; if it will not remove
   cleanly, say why in the comment):
   cd "$REPO" && git -C "$REPO" worktree remove "$WT"
   board comment $BOARD_CARD_ID "<PR url; checks passed/failed; mergeable state; anything the merger must know>"
   board done $BOARD_CARD_ID --outcome ok
   On failure: comment why, then `board done $BOARD_CARD_ID --outcome fail --summary "<why>"`.
Never run destructive git (reset --hard, push --force, branch -D) and never touch $BASE directly."##
    )
}

fn stage(parts: &[&str]) -> Option<String> {
    Some(parts.join("\n\n"))
}

struct Lane {
    harness: &'static str,
    model: &'static str,
    effort: &'static str,
    permission: &'static str,
}

const CODEX_ASTRA: Lane = Lane {
    harness: "codex",
    model: "gpt-6-astra",
    effort: "xhigh",
    permission: "full-access",
};
const CLAUDE_FABLE: Lane = Lane {
    harness: "claude",
    model: "fable",
    effort: "xhigh",
    permission: "auto",
};
const CLAUDE_OPUS: Lane = Lane {
    harness: "claude",
    model: "claude-opus-5-5",
    effort: "xhigh",
    permission: "auto",
};
const CODEX_SOL: Lane = Lane {
    harness: "codex",
    model: "gpt-6-sol",
    effort: "xhigh",
    permission: "full-access",
};
const CLAUDE_OPUS_MEDIUM: Lane = Lane {
    harness: "claude",
    model: "claude-opus-5-5",
    effort: "medium",
    permission: "auto",
};

fn agent_column(
    board_id: i64,
    name: &str,
    prompt: Option<String>,
    lane: Lane,
) -> ColumnCreateParams {
    ColumnCreateParams {
        name: name.into(),
        board_id: Some(board_id),
        trigger: Some(Trigger::Auto),
        system_prompt: prompt,
        harness_override: Some(lane.harness.into()),
        model_override: Some(lane.model.into()),
        effort_override: Some(lane.effort.into()),
        permission_override: Some(lane.permission.into()),
        ..Default::default()
    }
}

fn manual_column(board_id: i64, name: &str) -> ColumnCreateParams {
    ColumnCreateParams {
        name: name.into(),
        board_id: Some(board_id),
        trigger: Some(Trigger::Manual),
        ..Default::default()
    }
}

/// The columns the `pipeline` template adds after the board's seed Todo column
/// (`todo_id`), and how they hand cards on. `scripts_dir` is the folder holding
/// `review-env.sh`; without it Review and Release skip the test-server steps.
pub fn pipeline(
    board_id: i64,
    todo_id: i64,
    scripts_dir: Option<&Path>,
) -> (Vec<ColumnCreateParams>, Vec<ColumnWiring>) {
    let scripts = scripts_dir.map(|dir| dir.to_string_lossy().into_owned());
    let scripts = scripts.as_deref();
    let specs = vec![
        agent_column(
            board_id,
            "Spec",
            stage(&[WORKTREE, &github_issue("grill"), SPEC]),
            CODEX_ASTRA,
        ),
        ColumnCreateParams {
            // A fresh session: Plan runs Claude and must not try to resume the
            // Codex conversation Spec left behind.
            fresh_session: Some(true),
            ..agent_column(
                board_id,
                "Plan",
                stage(&[WORKTREE, &github_issue("plan"), PLAN]),
                CLAUDE_FABLE,
            )
        },
        agent_column(
            board_id,
            "Execute",
            stage(&[WORKTREE, EXECUTE]),
            CLAUDE_OPUS,
        ),
        agent_column(
            board_id,
            "Review",
            stage(&[WORKTREE, &review(scripts)]),
            CODEX_SOL,
        ),
        manual_column(board_id, "Human Review"),
        ColumnCreateParams {
            timeout_minutes: Some(60),
            ..agent_column(
                board_id,
                "Release",
                stage(&[WORKTREE, &release(scripts)]),
                CLAUDE_OPUS_MEDIUM,
            )
        },
        manual_column(board_id, "Done"),
    ];
    let to = |index| Some(ColumnTarget::Created(index));
    let todo = Some(ColumnTarget::Existing(todo_id));
    let wiring = vec![
        // Spec: agreed → Plan; dropped → Todo.
        ColumnWiring {
            column_index: 0,
            on_success: to(1),
            on_fail: todo,
        },
        // Plan: → Execute; refused (issue taken) → Todo.
        ColumnWiring {
            column_index: 1,
            on_success: to(2),
            on_fail: todo,
        },
        // Execute → Review.
        ColumnWiring {
            column_index: 2,
            on_success: to(3),
            on_fail: None,
        },
        // Review: OK → Human Review; FAIL → back to Execute.
        ColumnWiring {
            column_index: 3,
            on_success: to(4),
            on_fail: to(2),
        },
        // Release: PR ready → Done; failed → back to Human Review.
        ColumnWiring {
            column_index: 5,
            on_success: to(6),
            on_fail: to(4),
        },
    ];
    (specs, wiring)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt<'a>(specs: &'a [ColumnCreateParams], name: &str) -> &'a str {
        specs
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.system_prompt.as_deref())
            .unwrap_or_else(|| panic!("{name} has a prompt"))
    }

    #[test]
    fn names_no_project_user_or_fixed_branch() {
        let (specs, _) = pipeline(1, 1, Some(Path::new("/opt/board/scripts")));
        for spec in &specs {
            let p = spec.system_prompt.as_deref().unwrap_or_default();
            for word in [
                "exceedo",
                "hemfrid",
                "/Users/",
                "origin/main",
                "--base main",
            ] {
                assert!(!p.contains(word), "{} mentions {word}", spec.name);
            }
        }
    }

    #[test]
    fn test_server_steps_follow_the_scripts_dir() {
        let (with, _) = pipeline(1, 1, Some(Path::new("/opt/board/scripts")));
        assert!(prompt(&with, "Review").contains("bash /opt/board/scripts/review-env.sh ensure"));
        assert!(prompt(&with, "Release").contains("bash /opt/board/scripts/review-env.sh stop"));
        let (without, _) = pipeline(1, 1, None);
        assert!(!prompt(&without, "Review").contains("review-env.sh"));
        assert!(!prompt(&without, "Release").contains("review-env.sh"));
    }

    #[test]
    fn every_agent_stage_starts_in_the_card_worktree() {
        let (specs, _) = pipeline(1, 1, None);
        for spec in specs.iter().filter(|c| c.trigger == Some(Trigger::Auto)) {
            assert!(
                prompt(&specs, &spec.name).starts_with("WORKTREE"),
                "{}",
                spec.name
            );
        }
    }
}
