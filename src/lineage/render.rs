//! Plain-text and structured lineage tree helpers.

use crate::db::{TraceKind, TraceNode};

/// Which side of the dual-pane lineage view is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineagePane {
    #[default]
    Ancestors,
    Descendants,
}

/// One visible row in a collapsible lineage tree.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TraceFlatRow {
    pub display: String,
    pub depth: usize,
    pub kind: TraceKind,
    pub path: Vec<usize>,
    pub has_children: bool,
    pub is_expanded: bool,
}

/// Compact `col=val` summary for a node (used by TUI + CLI).
pub fn format_node_summary(node: &TraceNode, max_cols: usize, max_val_chars: usize) -> String {
    let mut summary = String::new();
    for (col, val) in node.columns.iter().zip(node.values.iter()).take(max_cols) {
        if !summary.is_empty() {
            summary.push_str(", ");
        }
        let mut v = val.clone();
        if v.chars().count() > max_val_chars {
            v = format!("{}…", v.chars().take(max_val_chars.saturating_sub(1)).collect::<String>());
        }
        summary.push_str(&format!("{}={}", col, v));
    }
    if node.columns.len() > max_cols {
        summary.push_str(", …");
    }
    summary
}

fn glyph(kind: TraceKind) -> &'static str {
    match kind {
        TraceKind::Root => "●",
        TraceKind::Parent => "▲",
        TraceKind::Child => "▼",
    }
}

/// Recursively flatten a subtree for collapsible display.
pub fn flatten_subtree(
    node: &TraceNode,
    prefix: &str,
    is_last: bool,
    is_root: bool,
    path: &[usize],
    expanded: &std::collections::HashSet<Vec<usize>>,
    out: &mut Vec<TraceFlatRow>,
) {
    let branch = if is_root {
        String::new()
    } else if is_last {
        format!("{}└─", prefix)
    } else {
        format!("{}├─", prefix)
    };

    let summary = format_node_summary(node, 5, 32);
    let has_children = !node.children.is_empty();
    let is_expanded = is_root || expanded.contains(path) || expanded.is_empty();
    // Default: fully expanded when expanded set is empty (first open).

    let mut display = format!("{}{} {}", branch, glyph(node.kind), node.table);
    if !node.via.is_empty() {
        display.push_str(&format!(" ({})", node.via));
    }
    if !summary.is_empty() {
        display.push_str(&format!("  {}", summary));
    }
    if let Some(ref note) = node.note {
        display.push_str(&format!("  [{}]", note));
    }
    if has_children && !is_root {
        let marker = if is_expanded { " ▼" } else { " ▶" };
        display.push_str(marker);
    }

    out.push(TraceFlatRow {
        display,
        depth: path.len(),
        kind: node.kind,
        path: path.to_vec(),
        has_children,
        is_expanded,
    });

    if !has_children || (!is_root && !is_expanded) {
        return;
    }

    let child_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };

    let count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let mut child_path = path.to_vec();
        child_path.push(i);
        flatten_subtree(
            child,
            &child_prefix,
            i == count - 1,
            false,
            &child_path,
            expanded,
            out,
        );
    }
}

/// Build ancestor-only and descendant-only roots from a full TraceNode.
pub fn split_lineage(root: &TraceNode) -> (TraceNode, TraceNode) {
    let parents: Vec<TraceNode> = root
        .children
        .iter()
        .filter(|c| c.kind == TraceKind::Parent)
        .cloned()
        .collect();
    let children: Vec<TraceNode> = root
        .children
        .iter()
        .filter(|c| c.kind == TraceKind::Child)
        .cloned()
        .collect();

    let ancestors = TraceNode {
        kind: TraceKind::Root,
        table: root.table.clone(),
        via: root.via.clone(),
        columns: root.columns.clone(),
        values: root.values.clone(),
        children: parents,
        note: root.note.clone(),
    };
    let descendants = TraceNode {
        kind: TraceKind::Root,
        table: root.table.clone(),
        via: root.via.clone(),
        columns: root.columns.clone(),
        values: root.values.clone(),
        children: children,
        note: root.note.clone(),
    };
    (ancestors, descendants)
}

/// Dual-section plain-text tree for CLI / AI agents.
pub fn render_dual_tree_text(root: &TraceNode) -> String {
    let (ancestors, descendants) = split_lineage(root);
    let mut out = String::new();

    out.push_str(&format!(
        "══ ASCENDENCIA (▲ parents) · {} ══\n",
        root.table
    ));
    render_tree_plain(&ancestors, "", true, true, &mut out);
    if ancestors.children.is_empty() {
        out.push_str("  (sin ancestros)\n");
    }

    out.push('\n');
    out.push_str(&format!(
        "══ DESCENDENCIA (▼ children) · {} ══\n",
        root.table
    ));
    render_tree_plain(&descendants, "", true, true, &mut out);
    if descendants.children.is_empty() {
        out.push_str("  (sin descendientes)\n");
    }

    out
}

fn render_tree_plain(node: &TraceNode, prefix: &str, is_last: bool, is_root: bool, out: &mut String) {
    let summary = format_node_summary(node, 6, 40);
    let branch = if is_root {
        String::new()
    } else if is_last {
        format!("{}└─", prefix)
    } else {
        format!("{}├─", prefix)
    };

    out.push_str(&branch);
    out.push_str(&format!("{} {}", glyph(node.kind), node.table));
    if !node.via.is_empty() {
        out.push_str(&format!(" ({})", node.via));
    }
    if !summary.is_empty() {
        out.push_str(&format!("  {}", summary));
    }
    if let Some(ref note) = node.note {
        out.push_str(&format!("  [{}]", note));
    }
    out.push('\n');

    let child_prefix = if is_root {
        String::new()
    } else if is_last {
        format!("{}   ", prefix)
    } else {
        format!("{}│  ", prefix)
    };
    let count = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        render_tree_plain(child, &child_prefix, i == count - 1, false, out);
    }
}

/// Styled line strings for the TUI (without ratatui dependency in this module).
pub fn render_trace_lines(root: &TraceNode, pane: LineagePane) -> Vec<String> {
    let (ancestors, descendants) = split_lineage(root);
    let side = match pane {
        LineagePane::Ancestors => &ancestors,
        LineagePane::Descendants => &descendants,
    };
    let expanded = std::collections::HashSet::new(); // fully expanded
    let mut rows = Vec::new();
    flatten_subtree(side, "", true, true, &[], &expanded, &mut rows);
    rows.into_iter().map(|r| r.display).collect()
}
