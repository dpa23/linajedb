//! `linajedb arbol <url> <tabla> <fila>` — headless lineage for AI agents.

use crate::db::{DbEngineConfig, DbRequest, DbResponse, DbWorker};
use crate::lineage::render_dual_tree_text;
use tokio::sync::mpsc;

fn config_from_url(url: &str) -> Result<DbEngineConfig, String> {
    if url.starts_with("mysql://") {
        Ok(DbEngineConfig::MariaDb {
            url: url.to_string(),
        })
    } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        Ok(DbEngineConfig::PostgreSql {
            url: url.to_string(),
        })
    } else if url.starts_with("mongodb://") || url.starts_with("mongodb+srv://") {
        let after_scheme = url.splitn(2, "://").nth(1).unwrap_or("");
        let database = after_scheme
            .splitn(2, '/')
            .nth(1)
            .map(|p| p.split('?').next().unwrap_or("").to_string())
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
                "MongoDB URL must include the database: mongodb://host:port/db".to_string()
            })?;
        Ok(DbEngineConfig::MongoDb {
            url: url.to_string(),
            database,
        })
    } else if url.starts_with("bolt://") || url.starts_with("neo4j://") {
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
        let path = url.strip_prefix("sqlite://").unwrap_or(url);
        Ok(DbEngineConfig::Sqlite {
            path: path.to_string(),
        })
    }
}

pub async fn run_arbol(url: &str, table: &str, condition: &str, json: bool) -> Result<(), String> {
    let (db_tx, mut db_rx) = mpsc::channel(100);
    let (app_tx, app_rx) = mpsc::channel(100);
    DbWorker::spawn(app_rx, db_tx);

    let send = |req: DbRequest| {
        let tx = app_tx.clone();
        async move { tx.send(req).await.map_err(|e| e.to_string()) }
    };

    send(DbRequest::Connect(config_from_url(url)?)).await?;
    match db_rx.recv().await {
        Some(DbResponse::Connected) => {}
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while connecting: {:?}", other)),
    }

    send(DbRequest::TraceStart {
        table: table.to_string(),
        condition: condition.to_string(),
    })
    .await?;

    let root = match db_rx.recv().await {
        Some(DbResponse::RowTrace(root)) => root,
        Some(DbResponse::Error(e)) => return Err(e),
        other => return Err(format!("unexpected response while tracing: {:?}", other)),
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&root.to_json()).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", render_dual_tree_text(&root));
    }
    Ok(())
}
