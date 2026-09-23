//! `A` in an open GitHub issue card assigns the issue to you on GitHub and
//! tags the card `mine`, so the Todo `Mine` filter shows it straight away.

use std::cell::RefCell;
use std::rc::Rc;

use board_core::client::BoardClient;
use board_core::protocol::CardCreateParams;
use board_tui::app::Screen;
use board_tui::github::IssueAssigner;
use board_tui::testkit::{demo_client, driver_with_editor, key};
use board_tui::Driver;
use crossterm::event::KeyCode;

const ISSUE_URL: &str = "https://github.com/acme/app/issues/42";

/// Records every issue it is asked to assign; fails when `error` is set.
#[derive(Clone, Default)]
struct FakeAssigner {
    assigned: Rc<RefCell<Vec<String>>>,
    error: Option<String>,
}

impl IssueAssigner for FakeAssigner {
    fn assign_to_me(&self, issue_url: &str) -> anyhow::Result<()> {
        if let Some(error) = &self.error {
            anyhow::bail!("{error}");
        }
        self.assigned.borrow_mut().push(issue_url.to_string());
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

    assert_eq!(*assigner.assigned.borrow(), [ISSUE_URL]);
    assert_eq!(tags_of(&d), ["github", "mine", "ready-for-agent"]);
    assert_eq!(toast(&d), "issue assigned to you");
}

#[test]
fn an_issue_already_yours_is_left_alone() {
    let assigner = FakeAssigner::default();
    let (mut d, _) = open_card(&["github", "mine"], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert!(assigner.assigned.borrow().is_empty());
    assert_eq!(toast(&d), "already assigned to you");
}

#[test]
fn a_card_not_made_from_an_issue_is_refused() {
    let assigner = FakeAssigner::default();
    let (mut d, _) = open_card(&[], &assigner);

    d.handle(key(KeyCode::Char('A')));

    assert!(assigner.assigned.borrow().is_empty());
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
