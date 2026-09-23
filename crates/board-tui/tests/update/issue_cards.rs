//! GitHub issue cards in the card detail view. `A` toggles you as the issue's
//! assignee on GitHub and the card's `mine` tag, so the Todo `Mine` filter
//! follows at once. `G` turns a hand-made card into a new issue assigned to
//! you.

use std::cell::RefCell;
use std::rc::Rc;

use board_core::client::BoardClient;
use board_core::protocol::CardCreateParams;
use board_tui::app::Screen;
use board_tui::github::GitHub;
use board_tui::testkit::{demo_client, driver_with_editor, key, render_at};
use board_tui::Driver;
use crossterm::event::KeyCode;

const ISSUE_URL: &str = "https://github.com/acme/app/issues/42";

/// Records every call; fails every call when `error` is set. New issues are
/// numbered from 100.
#[derive(Clone, Default)]
struct FakeGitHub {
    /// `set_me_assigned` calls as `(issue url, assign)`.
    calls: Rc<RefCell<Vec<(String, bool)>>>,
    /// `create_issue` calls as `(repo, title, body)`.
    created: Rc<RefCell<Vec<(String, String, String)>>>,
    error: Option<String>,
}

impl GitHub for FakeGitHub {
    fn set_me_assigned(&self, issue_url: &str, assign: bool) -> anyhow::Result<()> {
        if let Some(error) = &self.error {
            anyhow::bail!("{error}");
        }
        self.calls
            .borrow_mut()
            .push((issue_url.to_string(), assign));
        Ok(())
    }

    fn create_issue(&self, repo: &str, title: &str, body: &str) -> anyhow::Result<String> {
        if let Some(error) = &self.error {
            anyhow::bail!("{error}");
        }
        let mut created = self.created.borrow_mut();
        created.push((repo.to_string(), title.to_string(), body.to_string()));
        Ok(format!(
            "https://github.com/{repo}/issues/{}",
            99 + created.len()
        ))
    }
}

/// A driver with one issue card for #42 in Todo, open in the detail view.
fn open_card(tags: &[&str], github: &FakeGitHub) -> (Driver, i64) {
    open(
        CardCreateParams {
            title: "#42 Fix the thing".into(),
            description: Some(format!("GitHub issue #42: {ISSUE_URL}\n\nBody.")),
            tags: Some(tags.iter().map(|t| t.to_string()).collect()),
            ..Default::default()
        },
        &[],
        github,
    )
}

/// A driver with `card` plus `others` in Todo, `card` open in the detail view.
fn open(card: CardCreateParams, others: &[CardCreateParams], github: &FakeGitHub) -> (Driver, i64) {
    let mut client = demo_client().unwrap();
    let todo = client.board_get().unwrap().columns[0].id;
    for other in others {
        client
            .card_create(&CardCreateParams {
                column_id: Some(todo),
                ..other.clone()
            })
            .unwrap();
    }
    let id = client
        .card_create(&CardCreateParams {
            column_id: Some(todo),
            ..card
        })
        .unwrap()
        .id;
    let mut d = driver_with_editor(client, "");
    d.set_github(Box::new(github.clone()));
    let index = d
        .app
        .cards_of(todo)
        .iter()
        .position(|c| c.id == id)
        .unwrap();
    for _ in 0..index {
        d.handle(key(KeyCode::Down));
    }
    d.handle(key(KeyCode::Enter));
    assert_eq!(d.app.screen, Screen::CardDetail);
    assert_eq!(d.app.detail.as_ref().unwrap().card.id, id);
    (d, id)
}

fn tags_of(d: &Driver) -> Vec<String> {
    d.app.detail.as_ref().unwrap().card.tags.clone()
}

fn toast(d: &Driver) -> String {
    d.app
        .toast
        .as_ref()
        .map(|t| t.text.clone())
        .unwrap_or_default()
}

#[test]
fn assigns_the_issue_and_tags_the_card_mine() {
    let assigner = FakeGitHub::default();
    let (mut d, _) = open_card(&["github", "ready-for-agent"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert_eq!(*assigner.calls.borrow(), [(ISSUE_URL.to_string(), true)]);
    assert_eq!(tags_of(&d), ["github", "mine", "ready-for-agent"]);
    assert_eq!(toast(&d), "issue assigned to you");
}

#[test]
fn an_issue_already_yours_is_unassigned_and_loses_mine() {
    let assigner = FakeGitHub::default();
    let (mut d, _) = open_card(&["github", "mine", "ready-for-agent"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert_eq!(*assigner.calls.borrow(), [(ISSUE_URL.to_string(), false)]);
    assert_eq!(tags_of(&d), ["github", "ready-for-agent"]);
    assert_eq!(toast(&d), "issue unassigned from you");
}

#[test]
fn pressing_a_twice_assigns_then_unassigns() {
    let assigner = FakeGitHub::default();
    let (mut d, _) = open_card(&["github"], &assigner);

    d.handle(key(KeyCode::Char('A')));
    d.handle(key(KeyCode::Char('A')));

    assert_eq!(
        *assigner.calls.borrow(),
        [
            (ISSUE_URL.to_string(), true),
            (ISSUE_URL.to_string(), false)
        ]
    );
    assert_eq!(tags_of(&d), ["github"]);
}

#[test]
fn a_card_not_made_from_an_issue_is_refused() {
    let assigner = FakeGitHub::default();
    let (mut d, _) = open_card(&[], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert!(assigner.calls.borrow().is_empty());
    assert_eq!(toast(&d), "not a GitHub issue card");
}

#[test]
fn a_github_failure_keeps_the_card_unchanged() {
    let assigner = FakeGitHub {
        error: Some("HTTP 403: no access".into()),
        ..Default::default()
    };
    let (mut d, _) = open_card(&["github"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert_eq!(tags_of(&d), ["github"]);
    assert_eq!(toast(&d), "could not assign the issue: HTTP 403: no access");
    assert!(d.app.toast.as_ref().unwrap().is_error);
}

#[test]
fn the_status_line_says_whether_the_issue_is_yours() {
    let assigner = FakeGitHub::default();
    let (mut d, _) = open_card(&["github"], &assigner);
    let before = render_at(&mut d, 120, 35);
    assert!(before.contains("· not yours"), "{before}");

    d.handle(key(KeyCode::Char('A')));
    let after = render_at(&mut d, 120, 35);
    assert!(
        after.contains("· yours") && !after.contains("not yours"),
        "{after}"
    );
}

#[test]
fn a_card_not_made_from_an_issue_shows_no_assignee() {
    let (mut d, _) = open_card(&[], &FakeGitHub::default());
    assert!(!render_at(&mut d, 120, 35).contains("yours"));
}

/// The board's issue card for #42 in acme/app, so `G` knows the repo.
fn synced_issue() -> CardCreateParams {
    CardCreateParams {
        title: "#42 Synced".into(),
        description: Some(format!("GitHub issue #42: {ISSUE_URL}")),
        tags: Some(vec!["github".into()]),
        ..Default::default()
    }
}

fn hand_made() -> CardCreateParams {
    CardCreateParams {
        title: "Speed up the list".into(),
        description: Some("It is slow.".into()),
        ..Default::default()
    }
}

#[test]
fn g_creates_an_issue_and_links_the_card_to_it() {
    let github = FakeGitHub::default();
    let (mut d, _) = open(hand_made(), &[synced_issue()], &github);

    d.handle(key(KeyCode::Char('G')));

    assert_eq!(
        *github.created.borrow(),
        [(
            "acme/app".to_string(),
            "Speed up the list".to_string(),
            "It is slow.".to_string()
        )]
    );
    let card = &d.app.detail.as_ref().unwrap().card;
    assert_eq!(card.title, "#100 Speed up the list");
    assert!(card
        .description
        .starts_with("GitHub issue #100: https://github.com/acme/app/issues/100\n"));
    assert!(card.description.ends_with("\n\nIt is slow."));
    assert_eq!(card.tags, ["github", "mine"]);
    assert_eq!(toast(&d), "created issue #100, assigned to you");
}

#[test]
fn g_on_an_issue_card_is_refused() {
    let github = FakeGitHub::default();
    let (mut d, _) = open_card(&["github"], &github);

    d.handle(key(KeyCode::Char('G')));

    assert!(github.created.borrow().is_empty());
    assert_eq!(toast(&d), "already a GitHub issue card");
}

#[test]
fn g_without_any_issue_card_on_the_board_is_refused() {
    let github = FakeGitHub::default();
    let (mut d, _) = open(hand_made(), &[], &github);

    d.handle(key(KeyCode::Char('G')));

    assert!(github.created.borrow().is_empty());
    assert_eq!(
        toast(&d),
        "no GitHub repo on this board yet: sync issues first"
    );
}

#[test]
fn a_failed_issue_creation_leaves_the_card_unchanged() {
    let github = FakeGitHub {
        error: Some("HTTP 422: validation failed".into()),
        ..Default::default()
    };
    let (mut d, _) = open(hand_made(), &[synced_issue()], &github);

    d.handle(key(KeyCode::Char('G')));

    let card = &d.app.detail.as_ref().unwrap().card;
    assert_eq!(card.title, "Speed up the list");
    assert!(card.tags.is_empty());
    assert_eq!(
        toast(&d),
        "could not create the issue: HTTP 422: validation failed"
    );
}
