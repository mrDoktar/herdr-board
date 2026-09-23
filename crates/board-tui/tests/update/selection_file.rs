//! The TUI publishes its selected card to a file, so a Herdr keybinding
//! outside the TUI can act on "the card I have selected".

use board_tui::testkit::{demo_driver, key};
use crossterm::event::KeyCode;
use serde_json::Value;

fn read(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn publishes_the_selected_card_and_follows_the_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tui-selection.json");
    let mut d = demo_driver("x");
    d.enable_selection_file(path.clone());

    d.publish_selection();
    let first = read(&path);
    assert_eq!(first["board_id"], d.app.board.board.id);
    assert_eq!(first["card_id"], d.app.selected_card_id().unwrap());

    d.handle(key(KeyCode::Char('l')));
    d.publish_selection();
    let second = read(&path);
    assert_eq!(second["card_id"], d.app.selected_card_id().unwrap());
    assert_ne!(second["card_id"], first["card_id"]);
}
