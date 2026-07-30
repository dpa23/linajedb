//! Messages produced by terminal input, consumed by `update`.
//!
//! Not wired up yet: the `run` loop still matches raw crossterm events
//! directly. Kept as the target shape for the Elm-style refactor.

use crossterm::event::KeyEvent;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Message {
    Key(KeyEvent),
    Click { column: u16, row: u16 },
    Scroll { forward: bool },
    Quit,
}
