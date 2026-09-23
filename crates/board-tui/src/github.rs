//! GitHub issue cards: cards `scripts/issue-sync.sh` made from an issue. The
//! `gh` CLI does the GitHub side, isolated behind a trait so tests can fake it.

use anyhow::{bail, Context};
use board_core::model::Card;

use crate::app::TagFilter;

/// The tag the issue sync gives a card whose issue is assigned to you.
pub const MINE_TAG: &str = "mine";

/// The issue a card was made from: the first
/// `https://github.com/<owner>/<repo>/issues/<n>` link in its description.
/// `None` unless the card carries the [`TagFilter::ISSUE_TAG`] tag.
pub fn issue_url(card: &Card) -> Option<&str> {
    if !card.tags.iter().any(|t| t == TagFilter::ISSUE_TAG) {
        return None;
    }
    card.description
        .split_whitespace()
        .find(|word| is_issue_url(word))
}

fn is_issue_url(word: &str) -> bool {
    let Some(path) = word.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = path.split('/').collect();
    matches!(parts.as_slice(), [owner, repo, "issues", number]
        if !owner.is_empty() && !repo.is_empty()
            && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The `owner/repo` of an issue link.
pub fn repo_of(issue_url: &str) -> Option<&str> {
    let path = issue_url.strip_prefix("https://github.com/")?;
    let end = path.find("/issues/")?;
    Some(&path[..end])
}

/// The issue number at the end of an issue link.
pub fn number_of(issue_url: &str) -> Option<u64> {
    issue_url.rsplit('/').next()?.parse().ok()
}

/// The GitHub side of issue cards.
pub trait GitHub {
    /// Add the signed-in GitHub user to the issue's assignees (`assign`), or
    /// remove them.
    fn set_me_assigned(&self, issue_url: &str, assign: bool) -> anyhow::Result<()>;

    /// Open an issue in `repo` (`owner/name`) assigned to the signed-in user;
    /// returns its link.
    fn create_issue(&self, repo: &str, title: &str, body: &str) -> anyhow::Result<String>;
}

/// Production implementation: the `gh` CLI. Blocks until `gh` returns (about
/// a second).
pub struct GhCli;

/// Run `gh` with `args`; its stdout, or its first stderr line as the error.
fn gh(args: &[&str]) -> anyhow::Result<String> {
    let out = std::process::Command::new("gh")
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .context("could not run gh")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("{}", stderr.lines().next().unwrap_or("gh failed").trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl GitHub for GhCli {
    fn set_me_assigned(&self, issue_url: &str, assign: bool) -> anyhow::Result<()> {
        let flag = if assign {
            "--add-assignee"
        } else {
            "--remove-assignee"
        };
        gh(&["issue", "edit", issue_url, flag, "@me"]).map(|_| ())
    }

    fn create_issue(&self, repo: &str, title: &str, body: &str) -> anyhow::Result<String> {
        let out = gh(&[
            "issue",
            "create",
            "--repo",
            repo,
            "--title",
            title,
            "--body",
            body,
            "--assignee",
            "@me",
        ])?;
        // `gh issue create` prints the new issue's link as its last line.
        out.lines()
            .map(str::trim)
            .rfind(|line| is_issue_url(line))
            .map(str::to_string)
            .context("gh did not print the new issue's link")
    }
}

#[cfg(test)]
mod tests {
    use super::{is_issue_url, number_of, repo_of};

    #[test]
    fn recognizes_only_issue_links() {
        assert!(is_issue_url("https://github.com/o/r/issues/12"));
        assert!(!is_issue_url("https://github.com/o/r/pull/12"));
        assert!(!is_issue_url("https://github.com/o/r/issues/12x"));
        assert!(!is_issue_url("https://github.com/o/r/issues/"));
        assert!(!is_issue_url("http://github.com/o/r/issues/12"));
    }

    #[test]
    fn splits_an_issue_link_into_repo_and_number() {
        let url = "https://github.com/acme/app/issues/42";
        assert_eq!(repo_of(url), Some("acme/app"));
        assert_eq!(number_of(url), Some(42));
    }
}
