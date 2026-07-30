//! Shared row-lineage tree rendering for TUI and CLI.
//!
//! Separates ASCENDENCIA (parents) from DESCENDENCIA (children) so the
//! graph is readable in both interactive and headless modes.

mod render;

pub use render::{render_dual_tree_text, render_trace_lines, LineagePane};
