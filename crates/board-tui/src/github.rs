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

/// Changes an issue on GitHub.
pub trait IssueAssigner {
    /// Add the signed-in GitHub user to the issue's assignees.
    fn assign_to_me(&self, issue_url: &str) -> anyhow::Result<()>;
}

/// Production assigner: `gh issue edit <url> --add-assignee @me`. Blocks
/// until `gh` returns (about a second).
pub struct GhCli;

impl IssueAssigner for GhCli {
    fn assign_to_me(&self, issue_url: &str) -> anyhow::Result<()> {
        let out = std::process::Command::new("gh")
            .args(["issue", "edit", issue_url, "--add-assignee", "@me"])
            .stdin(std::process::Stdio::null())
            .output()
            .context("could not run gh")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            bail!("{}", stderr.lines().next().unwrap_or("gh failed").trim());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::is_issue_url;

    #[test]
    fn recognizes_only_issue_links() {
        assert!(is_issue_url("https://github.com/o/r/issues/12"));
        assert!(!is_issue_url("https://github.com/o/r/pull/12"));
        assert!(!is_issue_url("https://github.com/o/r/issues/12x"));
        assert!(!is_issue_url("https://github.com/o/r/issues/"));
        assert!(!is_issue_url("http://github.com/o/r/issues/12"));
    }
}
