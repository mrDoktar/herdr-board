//! `A` in an open GitHub issue card toggles you as the issue's assignee on
//! GitHub and the card's `mine` tag, so the Todo `Mine` filter follows at once.

use std::cell::RefCell;
use std::rc::Rc;

use board_core::client::BoardClient;
use board_core::protocol::CardCreateParams;
use board_tui::app::Screen;
use board_tui::github::IssueAssigner;
use board_tui::testkit::{demo_client, driver_with_editor, key, render_at};
use board_tui::Driver;
use crossterm::event::KeyCode;

const ISSUE_URL: &str = "https://github.com/acme/app/issues/42";

/// Records every call as `(issue url, assign)`; fails when `error` is set.
#[derive(Clone, Default)]
struct FakeAssigner {
    calls: Rc<RefCell<Vec<(String, bool)>>>,
    error: Option<String>,
}

impl IssueAssigner for FakeAssigner {
    fn set_me_assigned(&self, issue_url: &str, assign: bool) -> anyhow::Result<()> {
        if let Some(error) = &self.error {
            anyhow::bail!("{error}");
        }
        self.calls
            .borrow_mut()
            .push((issue_url.to_string(), assign));
        Ok(())
    }
}

/// A driver with one card in Todo, open in the detail view.
fn open_card(tags: &[&str], assigner: &FakeAssigner) -> (Driver, i64) {
    let mut client = demo_client().unwrap();
    let todo = client.board_get().unwrap().columns[0].id;
    let id = client
        .card_create(&CardCreateParams {
            title: "#42 Fix the thing".into(),
            description: Some(format!("GitHub issue #42: {ISSUE_URL}\n\nBody.")),
            column_id: Some(todo),
            tags: Some(tags.iter().map(|t| t.to_string()).collect()),
            ..Default::default()
        })
        .unwrap()
        .id;
    let mut d = driver_with_editor(client, "");
    d.set_issue_assigner(Box::new(assigner.clone()));
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
    let assigner = FakeAssigner::default();
    let (mut d, _) = open_card(&["github", "ready-for-agent"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert_eq!(*assigner.calls.borrow(), [(ISSUE_URL.to_string(), true)]);
    assert_eq!(tags_of(&d), ["github", "mine", "ready-for-agent"]);
    assert_eq!(toast(&d), "issue assigned to you");
}

#[test]
fn an_issue_already_yours_is_unassigned_and_loses_mine() {
    let assigner = FakeAssigner::default();
    let (mut d, _) = open_card(&["github", "mine", "ready-for-agent"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert_eq!(*assigner.calls.borrow(), [(ISSUE_URL.to_string(), false)]);
    assert_eq!(tags_of(&d), ["github", "ready-for-agent"]);
    assert_eq!(toast(&d), "issue unassigned from you");
}

#[test]
fn pressing_a_twice_assigns_then_unassigns() {
    let assigner = FakeAssigner::default();
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
    let assigner = FakeAssigner::default();
    let (mut d, _) = open_card(&[], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert!(assigner.calls.borrow().is_empty());
    assert_eq!(toast(&d), "not a GitHub issue card");
}

#[test]
fn a_github_failure_keeps_the_card_unchanged() {
    let assigner = FakeAssigner {
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
    let assigner = FakeAssigner::default();
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
    let (mut d, _) = open_card(&[], &FakeAssigner::default());
    assert!(!render_at(&mut d, 120, 35).contains("yours"));
}
