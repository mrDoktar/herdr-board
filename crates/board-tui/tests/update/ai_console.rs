//! Jumping from the board straight to a card's AI console (its newest run's
//! Herdr pane): `o`, Ctrl+Enter, and a click on the card's `[▶]`. Enter and a
//! double-click still open the card.

use super::helpers::{demo_app, key};
use board_core::protocol::CardStatus;
use board_tui::app::{update, App, Effect, Msg, Screen};
use board_tui::testkit::{demo_driver, render_at};
use board_tui::widgets::Zone;
use board_tui::Driver;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

fn jumps_to_selected(app: &App, effects: &[Effect]) -> bool {
    let selected = app
        .selected_card_id()
        .expect("the demo board selects a card");
    matches!(effects, [Effect::FocusLatestRun(id)] if *id == selected)
}

#[test]
fn o_jumps_to_the_ai_console() {
    let mut app = demo_app();
    let effects = update(&mut app, key(KeyCode::Char('o')));
    assert!(jumps_to_selected(&app, &effects));
    assert_eq!(app.screen, Screen::Board);
}

#[test]
fn ctrl_enter_jumps_but_enter_opens_the_card() {
    let mut app = demo_app();
    let ctrl_enter = Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let effects = update(&mut app, ctrl_enter);
    assert!(jumps_to_selected(&app, &effects));

    update(&mut app, key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::CardDetail);
}

fn left_click(column: u16, row: u16) -> Msg {
    Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// Every `[▶]` the last draw registered, as `(card id, x, y)`.
fn console_buttons(d: &mut Driver) -> Vec<(i64, u16, u16)> {
    render_at(d, 120, 35);
    let area = d.app.last_area;
    let hit_map = d.app.hit_map.borrow();
    let mut found: Vec<(i64, u16, u16)> = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some(Zone::CardConsole(id)) = hit_map.hit(x, y) {
                if !found.iter().any(|(seen, _, _)| *seen == id) {
                    found.push((id, x, y));
                }
            }
        }
    }
    found
}

#[test]
fn clicking_a_cards_console_button_jumps_to_its_ai_console() {
    let mut d = demo_driver("");
    let (id, x, y) = *console_buttons(&mut d)
        .first()
        .expect("the demo board has a card that ran");
    let effects = update(&mut d.app, left_click(x, y));
    assert!(matches!(effects.as_slice(), [Effect::FocusLatestRun(got)] if *got == id));
    assert_eq!(d.app.screen, Screen::Board);
}

#[test]
fn only_cards_that_ran_get_a_console_button() {
    let mut d = demo_driver("");
    for (id, _, _) in console_buttons(&mut d) {
        let card = d.app.board.cards.iter().find(|c| c.id == id).unwrap();
        assert_ne!(card.status, CardStatus::Idle, "idle card {id} has a [▶]");
        assert_ne!(
            card.status,
            CardStatus::Queued,
            "queued card {id} has a [▶]"
        );
    }
}

#[test]
fn a_double_click_opens_the_card() {
    let mut app = demo_app();
    let layout = board_tui::view::board_layout(&app, app.last_area);
    let (x, y) = (0..app.last_area.height)
        .flat_map(|y| (0..app.last_area.width).map(move |x| (x, y)))
        .find(|&(x, y)| layout.hit_card(x, y) == Some((app.sel_col, app.sel_card)))
        .expect("the selected card is on screen");
    update(&mut app, left_click(x, y));
    update(&mut app, left_click(x, y));
    assert_eq!(app.screen, Screen::CardDetail);
}
