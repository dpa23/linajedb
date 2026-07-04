//! Headless (non-interactive) subcommands, for scripts and AI agents.
//!
//! `db-tui trace` reuses the same DbWorker the TUI uses, but prints the
//! row-lineage tree to stdout (JSON by default) instead of drawing panes.

use crate::db::{DbEngineConfig, DbRequest, DbResponse, DbWorker, TraceKind, TraceNode};
use tokio::sync::mpsc;

const USAGE: &str = "\
Usage: db-tui trace --url <URL> --table <TABLE> --where <SQL-CONDITION> [--format json|tree]

Walks the foreign-key graph from one row in both directions (ancestors of
ancestors, children of children) and prints the lineage.

Options:
  --url <URL>        Connection URL: mysql://user:pass@host:port/db,
                     postgres://user:pass@host:port/db, or a SQLite file path.
  --table <TABLE>    Table the starting row lives in.
  --where <COND>     SQL condition selecting the starting row, e.g. \"id=42\".
                     If it matches several rows, the first one is traced.
  --format <FMT>     json (default) or tree.

Example:
  db-tui trace --url mysql://root:pw@127.0.0.1:3306/shop \\
      --table orders --where \"id_order=118\" --format tree";

struct TraceArgs {
    url: String,
    table: String,
    condition: String,
    json: bool,
}

fn parse_args(args: &[String]) -> Result<TraceArgs, String> {
    let mut url = None;
    let mut table = None;
    let mut condition = None;
    let mut json = true;

    let mut it = args.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--help" | "-h" => return Err(USAGE.to_string()),
            "--url" => url = it.next().cloned(),
            "--table" => table = it.next().cloned(),
            "--where" => condition = it.next().cloned(),
            "--format" => match it.next().map(|s| s.as_str()) {
                Some("json") => json = true,
                Some("tree") => json = false,
                other => return Err(format!("--format must be json or tree, got {:?}\n\n{}", other, USAGE)),
            },
            other => return Err(format!("unknown flag: {}\n\n{}", other, USAGE)),
        }
    }

    Ok(TraceArgs {
        url: url.ok_or_else(|| format!("missing --url\n\n{}", USAGE))?,
        table: table.ok_or_else(|| format!("missing --table\n\n{}", USAGE))?,
        condition: condition.ok_or_else(|| format!("missing --where\n\n{}", USAGE))?,
        json,
    })
}

fn config_from_url(url: &str) -> DbEngineConfig {
    if url.starts_with("mysql://") {
        DbEngineConfig::MariaDb { url: url.to_string() }
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        DbEngineConfig::PostgreSql { url: url.to_string() }
    } else {
        // Anything else is treated as a SQLite file path.
        let path = url.strip_prefix("sqlite://").unwrap_or(url);
        DbEngineConfig::Sqlite { path: path.to_string() }
    }
}

pub async fn run_trace(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", USAGE);
        return Ok(());
    }
    let args = parse_args(args)?;

    let (db_tx, mut db_rx) = mpsc::channel(100);
    let (app_tx, app_rx) = mpsc::channel(100);
    DbWorker::spawn(app_rx, db_tx);

    let send = |req: DbRequest| {
        let tx = app_tx.clone();
        async move { tx.send(req).await.map_err(|e| e.to_string()) }
    };

    send(DbRequest::Connect(config_from_url(&args.url))).await?;
    match db_rx.recv().await {
        Some(DbResponse::Connected) => {}
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while connecting: {:?}", other)),
    }

    let query = format!(
        "SELECT * FROM {} WHERE {} LIMIT 1;",
        args.table, args.condition
    );
    send(DbRequest::ExecuteQuery(query)).await?;
    let (columns, rows) = match db_rx.recv().await {
        Some(DbResponse::QueryResult { columns, rows }) => (columns, rows),
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while selecting the row: {:?}", other)),
    };
    // Empty results come back as a single "Status" pseudo-row.
    if rows.is_empty() || (columns.len() == 1 && columns[0] == "Status") {
        return Err(format!(
            "no row in {} matches: {}",
            args.table, args.condition
        ));
    }

    send(DbRequest::TraceRow {
        table: args.table,
        columns,
        values: rows[0].clone(),
    })
    .await?;
    let root = match db_rx.recv().await {
        Some(DbResponse::RowTrace(root)) => root,
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while tracing: {:?}", other)),
    };

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&root.to_json()).map_err(|e| e.to_string())?
        );
    } else {
        let mut out = String::new();
        render_tree(&root, "", true, true, &mut out);
        print!("{}", out);
    }
    Ok(())
}

/// Plain-text sibling of the TUI tree renderer (ui::render_trace_node).
fn render_tree(node: &TraceNode, prefix: &str, is_last: bool, is_root: bool, out: &mut String) {
    let glyph = match node.kind {
        TraceKind::Root => "●",
        TraceKind::Parent => "▲",
        TraceKind::Child => "▼",
    };

    let mut summary = String::new();
    for (col, val) in node.columns.iter().zip(node.values.iter()).take(6) {
        if !summary.is_empty() {
            summary.push_str(", ");
        }
        let mut v = val.clone();
        if v.chars().count() > 40 {
            v = format!("{}…", v.chars().take(39).collect::<String>());
        }
        summary.push_str(&format!("{}={}", col, v));
    }
    if node.columns.len() > 6 {
        summary.push_str(", …");
    }

    let branch = if is_root {
        String::new()
    } else if is_last {
        format!("{}└─", prefix)
    } else {
        format!("{}├─", prefix)
    };

    out.push_str(&branch);
    out.push_str(&format!("{} {}", glyph, node.table));
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
        render_tree(child, &child_prefix, i == count - 1, false, out);
    }
}
