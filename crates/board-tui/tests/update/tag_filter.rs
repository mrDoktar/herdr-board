//! `t` cycles which GitHub issue cards the Todo column shows: all, the ones
//! ready for an agent, or the ones assigned to you. Cards not made from an
//! issue always show, and other columns are never filtered.

use board_core::client::BoardClient;
use board_core::protocol::CardCreateParams;
use board_tui::app::TagFilter;
use board_tui::testkit::{demo_client, driver_with_editor, key};
use board_tui::Driver;
use crossterm::event::KeyCode;

fn issue_card(title: &str, column_id: i64, tags: &[&str]) -> CardCreateParams {
    CardCreateParams {
        title: title.into(),
        column_id: Some(column_id),
        tags: Some(tags.iter().map(|t| t.to_string()).collect()),
        ..Default::default()
    }
}

/// The demo board plus three issue cards in Todo and one in Plan.
fn setup() -> (Driver, i64, i64) {
    let mut client = demo_client().unwrap();
    let board = client.board_get().unwrap();
    let column = |name: &str| board.columns.iter().find(|c| c.name == name).unwrap().id;
    let (todo, plan) = (column("Todo"), column("Plan"));
    for (title, tags) in [
        ("#1 ready", &["github", "ready-for-agent"][..]),
        ("#2 mine", &["github", "mine"][..]),
        ("#3 other", &["github"][..]),
    ] {
        client.card_create(&issue_card(title, todo, tags)).unwrap();
    }
    client
        .card_create(&issue_card("#4 planned", plan, &["github"]))
        .unwrap();
    (driver_with_editor(client, ""), todo, plan)
}

fn titles(d: &Driver, column_id: i64) -> Vec<String> {
    d.app
        .cards_of(column_id)
        .iter()
        .map(|c| c.title.clone())
        .collect()
}

fn issue_titles(d: &Driver, column_id: i64) -> Vec<String> {
    titles(d, column_id)
        .into_iter()
        .filter(|t| t.starts_with('#'))
        .collect()
}

#[test]
fn todo_shows_every_issue_card_by_default() {
    let (d, todo, _) = setup();
    assert_eq!(d.app.tag_filter, TagFilter::All);
    assert_eq!(issue_titles(&d, todo), ["#1 ready", "#2 mine", "#3 other"]);
}

#[test]
fn t_cycles_ready_for_agent_then_mine_then_all() {
    let (mut d, todo, _) = setup();

    d.handle(key(KeyCode::Char('t')));
    assert_eq!(d.app.tag_filter, TagFilter::ReadyForAgent);
    assert_eq!(issue_titles(&d, todo), ["#1 ready"]);

    d.handle(key(KeyCode::Char('t')));
    assert_eq!(d.app.tag_filter, TagFilter::Mine);
    assert_eq!(issue_titles(&d, todo), ["#2 mine"]);

    d.handle(key(KeyCode::Char('t')));
    assert_eq!(d.app.tag_filter, TagFilter::All);
    assert_eq!(issue_titles(&d, todo), ["#1 ready", "#2 mine", "#3 other"]);
}

#[test]
fn filter_keeps_hand_made_cards_and_leaves_other_columns_alone() {
    let (mut d, todo, plan) = setup();
    let hand_made: Vec<String> = titles(&d, todo)
        .into_iter()
        .filter(|t| !t.starts_with('#'))
        .collect();
    let plan_before = titles(&d, plan);

    d.handle(key(KeyCode::Char('t')));
    d.handle(key(KeyCode::Char('t')));
    assert_eq!(d.app.tag_filter, TagFilter::Mine);

    for title in &hand_made {
        assert!(titles(&d, todo).contains(title), "{title} was hidden");
    }
    assert_eq!(titles(&d, plan), plan_before);
    assert!(plan_before.contains(&"#4 planned".to_string()));
}
