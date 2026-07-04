//! Headless (non-interactive) subcommands, for scripts and AI agents.
//!
//! `linajedb trace` reuses the same DbWorker the TUI uses, but prints the
//! row-lineage tree to stdout (JSON by default) instead of drawing panes.

use crate::db::{DbEngineConfig, DbRequest, DbResponse, DbWorker, TraceKind, TraceNode};
use tokio::sync::mpsc;

const USAGE: &str = "\
Usage: linajedb trace --url <URL> --table <TABLE> --where <CONDITION> [--format json|tree]

Walks the relationship graph from one row/document/node in both directions
(ancestors of ancestors, children of children) and prints the lineage.

Options:
  --url <URL>        mysql://user:pass@host:port/db
                     postgres://user:pass@host:port/db
                     mongodb://host:port/db  (db in the path is required)
                     bolt://user:pass@host:port  (Neo4j)
                     or a SQLite file path.
  --table <TABLE>    Table (relational), collection (MongoDB) or node label
                     (Neo4j) the starting point lives in.
  --where <COND>     Relational: SQL condition, e.g. \"id=42\".
                     MongoDB:    JSON filter, e.g. '{\"reference\": \"A-1\"}'.
                     Neo4j:      prop=value, or a raw condition using n.
                     If it matches several rows, the first one is traced.
  --format <FMT>     json (default) or tree.

Relational engines follow declared foreign keys; MongoDB infers references
by naming convention (user_id / id_user / userId -> collection user(s));
Neo4j follows real edges (outgoing = parents, incoming = children).

Example:
  linajedb trace --url mysql://root:pw@127.0.0.1:3306/shop \\
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

fn config_from_url(url: &str) -> Result<DbEngineConfig, String> {
    if url.starts_with("mysql://") {
        Ok(DbEngineConfig::MariaDb { url: url.to_string() })
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(DbEngineConfig::PostgreSql { url: url.to_string() })
    } else if url.starts_with("mongodb://") || url.starts_with("mongodb+srv://") {
        // The trace needs a concrete database: take it from the URL path.
        let after_scheme = url.splitn(2, "://").nth(1).unwrap_or("");
        let database = after_scheme
            .splitn(2, '/')
            .nth(1)
            .map(|p| p.split('?').next().unwrap_or("").to_string())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
                format!("MongoDB URL must include the database: mongodb://host:port/db\n\n{}", USAGE)
            })?;
        Ok(DbEngineConfig::MongoDb { url: url.to_string(), database })
    } else if url.starts_with("bolt://") || url.starts_with("neo4j://") {
        // Credentials travel in the URL: bolt://user:pass@host:port
        let (scheme, rest) = url.split_once("://").unwrap_or(("bolt", url));
        let (user, pass, host) = match rest.rsplit_once('@') {
            Some((creds, host)) => {
                let (u, p) = creds.split_once(':').unwrap_or((creds, ""));
                (u.to_string(), p.to_string(), host.to_string())
            }
            None => ("neo4j".to_string(), "neo4j".to_string(), rest.to_string()),
        };
        Ok(DbEngineConfig::Neo4j {
            url: format!("{}://{}", scheme, host),
            user,
            pass,
        })
    } else {
        // Anything else is treated as a SQLite file path.
        let path = url.strip_prefix("sqlite://").unwrap_or(url);
        Ok(DbEngineConfig::Sqlite { path: path.to_string() })
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

    send(DbRequest::Connect(config_from_url(&args.url)?)).await?;
    match db_rx.recv().await {
        Some(DbResponse::Connected) => {}
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while connecting: {:?}", other)),
    }

    send(DbRequest::TraceStart {
        table: args.table,
        condition: args.condition,
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
