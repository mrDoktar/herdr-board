//! Jumping from the board straight to a card's AI console (its newest run's
//! Herdr pane): `o`, Ctrl+Enter, and Ctrl/Alt+double-click. Plain Enter and a
//! plain double-click still open the card.

use super::helpers::{demo_app, key};
use board_tui::app::{update, App, Effect, Msg, Screen};
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

/// A point inside the currently selected card.
fn selected_card_point(app: &App) -> (u16, u16) {
    let layout = board_tui::view::board_layout(app, app.last_area);
    (0..app.last_area.height)
        .flat_map(|y| (0..app.last_area.width).map(move |x| (x, y)))
        .find(|&(x, y)| layout.hit_card(x, y) == Some((app.sel_col, app.sel_card)))
        .expect("the selected card is on screen")
}

fn double_click(app: &mut App, modifiers: KeyModifiers) -> Vec<Effect> {
    let (column, row) = selected_card_point(app);
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers,
    };
    update(app, Msg::Mouse(click));
    update(app, Msg::Mouse(click))
}

#[test]
fn ctrl_or_alt_double_click_jumps_to_the_ai_console() {
    for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
        let mut app = demo_app();
        let effects = double_click(&mut app, modifiers);
        assert!(jumps_to_selected(&app, &effects), "{modifiers:?}");
        assert_eq!(app.screen, Screen::Board);
    }
}

#[test]
fn a_plain_double_click_still_opens_the_card() {
    let mut app = demo_app();
    double_click(&mut app, KeyModifiers::NONE);
    assert_eq!(app.screen, Screen::CardDetail);
}
