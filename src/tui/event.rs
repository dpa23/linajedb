//! Terminal event input.
//!
//! Events are currently polled inline in the `run` loop of `tui::mod`
//! (`crossterm::event::poll` + `read`). This module is the future home of a
//! dedicated event source that turns raw crossterm events into
//! [`crate::tui::message::Message`] values.
