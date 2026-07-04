pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod nosql;
pub mod graph;
pub mod config;

use tokio::sync::mpsc;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone)]
pub enum DbEngineConfig {
    MariaDb { url: String },
    PostgreSql { url: String },
    Sqlite { path: String },
    MongoDb { url: String, database: String },
    Neo4j { url: String, user: String, pass: String },
    LocalJson { path: String },
}

#[derive(Clone, Debug)]
pub struct RelationshipInfo {
    pub is_parent: bool,
    pub target_table: String,
    pub active_col: String,
    pub target_col: String,
}

#[derive(Debug, Clone)]
pub enum DbRequest {
    Connect(DbEngineConfig),
    LoadTables,
    ExecuteQuery(String),
    LoadMetadata { table: String },
    LoadRelatedData { relationship: RelationshipInfo, active_row_val: String },
    TraceRow { table: String, columns: Vec<String>, values: Vec<String> },
    /// Engine-agnostic trace entry point (used by headless mode): find the
    /// starting row/document/node from a condition, then walk its lineage.
    /// `condition` is a SQL condition (relational), a JSON filter (MongoDB)
    /// or a `prop=value` / raw `n.`-prefixed Cypher condition (Neo4j).
    TraceStart { table: String, condition: String },
    LoadDatabases,
    SelectDatabase(String),
}

/// Direction of a node in a row-trace tree relative to its parent node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceKind {
    Root,
    Parent,
    Child,
}

/// One row in the lineage tree of a traced row. `children` holds the next
/// hop in the same direction: ancestors of a Parent node, descendants of a
/// Child node; the Root node carries both (Parents first, then Children).
#[derive(Debug, Clone)]
pub struct TraceNode {
    pub kind: TraceKind,
    pub table: String,
    /// FK that led here, e.g. "orders.customer_id = customers.id". Empty for the root.
    pub via: String,
    pub columns: Vec<String>,
    pub values: Vec<String>,
    pub children: Vec<TraceNode>,
    /// Why the walk stopped here (cycle, depth cap, extra rows, error).
    pub note: Option<String>,
}

impl TraceNode {
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }

    pub fn to_json(&self) -> Value {
        let mut row = serde_json::Map::new();
        for (col, val) in self.columns.iter().zip(self.values.iter()) {
            row.insert(col.clone(), cell_to_json(val));
        }
        let mut obj = serde_json::Map::new();
        obj.insert("table".to_string(), Value::String(self.table.clone()));
        if !self.via.is_empty() {
            obj.insert("via".to_string(), Value::String(self.via.clone()));
        }
        if let Some(ref note) = self.note {
            obj.insert("note".to_string(), Value::String(note.clone()));
        }
        obj.insert("row".to_string(), Value::Object(row));
        let parents: Vec<Value> = self.children.iter()
            .filter(|c| c.kind == TraceKind::Parent)
            .map(|c| c.to_json())
            .collect();
        let children: Vec<Value> = self.children.iter()
            .filter(|c| c.kind == TraceKind::Child)
            .map(|c| c.to_json())
            .collect();
        if !parents.is_empty() {
            obj.insert("parents".to_string(), Value::Array(parents));
        }
        if !children.is_empty() {
            obj.insert("children".to_string(), Value::Array(children));
        }
        Value::Object(obj)
    }
}

fn cell_to_json(val: &str) -> Value {
    if val == "NULL" {
        return Value::Null;
    }
    if let Ok(n) = val.parse::<i64>() {
        return Value::Number(n.into());
    }
    if let Ok(f) = val.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return Value::Number(n);
        }
    }
    Value::String(val.to_string())
}

#[derive(Debug, Clone)]
pub enum DbResponse {
    Connected,
    Tables(Vec<String>),
    QueryResult {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    DocumentResult(Vec<Value>),
    Metadata {
        primary_key: Option<String>,
        relationships: Vec<RelationshipInfo>,
    },
    RelatedData {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    RowTrace(TraceNode),
    Databases(Vec<String>),
    DatabaseSelected,
    Error(String),
}

enum ActiveConnection {
    None,
    MariaDb(sqlx::MySqlPool),
    PostgreSql(sqlx::PgPool),
    Sqlite(sqlx::SqlitePool),
    MongoDb { client: mongodb::Client, database: String },
    Neo4j(neo4rs::Graph),
    LocalJson { path: String },
}

pub struct DbWorker {
    request_rx: mpsc::Receiver<DbRequest>,
    response_tx: mpsc::Sender<DbResponse>,
    connection: ActiveConnection,
    active_config: Option<DbEngineConfig>,
}

impl DbWorker {
    pub fn spawn(
        request_rx: mpsc::Receiver<DbRequest>,
        response_tx: mpsc::Sender<DbResponse>,
    ) {
        let worker = DbWorker {
            request_rx,
            response_tx,
            connection: ActiveConnection::None,
            active_config: None,
        };
        tokio::spawn(async move {
            let mut w = worker;
            w.run().await;
        });
    }

    async fn run(&mut self) {
        while let Some(req) = self.request_rx.recv().await {
            let response = match req {
                DbRequest::Connect(config) => self.handle_connect(config).await,
                DbRequest::LoadTables => self.handle_load_tables().await,
                DbRequest::ExecuteQuery(query) => self.handle_execute_query(&query).await,
                DbRequest::LoadMetadata { table } => self.handle_load_metadata(&table).await,
                DbRequest::LoadRelatedData { relationship, active_row_val } => self.handle_load_related_data(relationship, active_row_val).await,
                DbRequest::TraceRow { table, columns, values } => self.handle_trace_row(table, columns, values).await,
                DbRequest::TraceStart { table, condition } => self.handle_trace_start(table, condition).await,
                DbRequest::LoadDatabases => self.handle_load_databases().await,
                DbRequest::SelectDatabase(db_name) => self.handle_select_database(db_name).await,
            };
            let _ = self.response_tx.send(response).await;
        }
    }

    async fn handle_connect(&mut self, config: DbEngineConfig) -> DbResponse {
        self.connection = ActiveConnection::None; // Close old connection
        self.active_config = Some(config.clone());
        match config {
            DbEngineConfig::MariaDb { url } => {
                match mysql::connect_mysql(&url).await {
                    Ok(pool) => {
                        self.connection = ActiveConnection::MariaDb(pool);
                        DbResponse::Connected
                    }
                    Err(e) => DbResponse::Error(format!("MySQL connection failed: {}", e)),
                }
            }
            DbEngineConfig::PostgreSql { url } => {
                match postgres::connect_postgres(&url).await {
                    Ok(pool) => {
                        self.connection = ActiveConnection::PostgreSql(pool);
                        DbResponse::Connected
                    }
                    Err(e) => DbResponse::Error(format!("Postgres connection failed: {}", e)),
                }
            }
            DbEngineConfig::Sqlite { path } => {
                match sqlite::connect_sqlite(&path).await {
                    Ok(pool) => {
                        self.connection = ActiveConnection::Sqlite(pool);
                        DbResponse::Connected
                    }
                    Err(e) => DbResponse::Error(format!("SQLite connection failed: {}", e)),
                }
            }
            DbEngineConfig::MongoDb { url, database } => {
                match nosql::connect_mongodb(&url).await {
                    Ok(client) => {
                        self.connection = ActiveConnection::MongoDb { client, database };
                        DbResponse::Connected
                    }
                    Err(e) => DbResponse::Error(format!("MongoDB connection failed: {}", e)),
                }
            }
            DbEngineConfig::Neo4j { url, user, pass } => {
                match graph::connect_neo4j(&url, &user, &pass).await {
                    Ok(graph_conn) => {
                        self.connection = ActiveConnection::Neo4j(graph_conn);
                        DbResponse::Connected
                    }
                    Err(e) => DbResponse::Error(format!("Neo4j connection failed: {}", e)),
                }
            }
            DbEngineConfig::LocalJson { path } => {
                // For local files, we just verify the file exists and is readable
                let path_buf = std::path::PathBuf::from(&path);
                if path_buf.exists() && path_buf.is_file() {
                    self.connection = ActiveConnection::LocalJson { path };
                    DbResponse::Connected
                } else {
                    DbResponse::Error(format!("JSON file not found or is directory: {}", path))
                }
            }
        }
    }

    async fn handle_load_tables(&self) -> DbResponse {
        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected to any database".to_string()),
            ActiveConnection::MariaDb(pool) => {
                match mysql::list_mysql_tables(pool).await {
                    Ok(tables) => DbResponse::Tables(tables),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::PostgreSql(pool) => {
                match postgres::list_postgres_tables(pool).await {
                    Ok(tables) => DbResponse::Tables(tables),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Sqlite(pool) => {
                match sqlite::list_sqlite_tables(pool).await {
                    Ok(tables) => DbResponse::Tables(tables),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::MongoDb { client, database } => {
                match nosql::list_mongodb_collections(client, database).await {
                    Ok(collections) => DbResponse::Tables(collections),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Neo4j(graph_conn) => {
                match graph::list_neo4j_labels(graph_conn).await {
                    Ok(labels) => DbResponse::Tables(labels),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::LocalJson { path } => {
                // Local JSON file acts as a single collection/table
                let file_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("local_file")
                    .to_string();
                DbResponse::Tables(vec![file_name])
            }
        }
    }

    async fn handle_execute_query(&self, query: &str) -> DbResponse {
        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected to any database".to_string()),
            ActiveConnection::MariaDb(pool) => {
                match mysql::execute_mysql_query(pool, query).await {
                    Ok((columns, rows)) => DbResponse::QueryResult { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::PostgreSql(pool) => {
                match postgres::execute_postgres_query(pool, query).await {
                    Ok((columns, rows)) => DbResponse::QueryResult { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Sqlite(pool) => {
                match sqlite::execute_sqlite_query(pool, query).await {
                    Ok((columns, rows)) => DbResponse::QueryResult { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::MongoDb { client, database } => {
                // We default collection to whatever is active or parsed from a custom syntax, 
                // but since the UI sidebar loads collections, the app state will pass the active collection 
                // as part of the query parameter or we assume query format is: collection_name|filter_json
                let parts: Vec<&str> = query.splitn(2, '|').collect();
                if parts.len() < 2 {
                    return DbResponse::Error("MongoDB query format must be: collection_name|filter_json".to_string());
                }
                let collection = parts[0];
                let filter = parts[1];
                match nosql::execute_mongodb_find(client, database, collection, filter, 100).await {
                    Ok(docs) => DbResponse::DocumentResult(docs),
                    Err(e) => DbResponse::Error(e),
                }
            }
            ActiveConnection::Neo4j(graph_conn) => {
                match graph::execute_neo4j_query(graph_conn, query).await {
                    Ok((columns, rows)) => DbResponse::QueryResult { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::LocalJson { path } => {
                if path.ends_with(".dbf") {
                    match read_local_dbf_file(path) {
                        Ok(resp) => resp,
                        Err(e) => DbResponse::Error(e),
                    }
                } else {
                    // Reads local file (JSON or BSON) and returns value
                    let res = if path.ends_with(".bson") {
                        nosql::read_local_bson_file(path)
                    } else {
                        nosql::read_local_json_file(path)
                    };

                    match res {
                        Ok(val) => {
                            // If it's an array of objects, return them as multiple documents.
                            // If it's a single object, return it in a single-item vector.
                            if let Value::Array(arr) = val {
                                DbResponse::DocumentResult(arr)
                            } else {
                                DbResponse::DocumentResult(vec![val])
                            }
                        }
                        Err(e) => DbResponse::Error(e),
                    }
                }
            }
        }
    }

    async fn handle_load_metadata(&self, table: &str) -> DbResponse {
        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected".to_string()),
            ActiveConnection::MariaDb(pool) => {
                match mysql::fetch_mysql_metadata(pool, table).await {
                    Ok((pk, rels)) => DbResponse::Metadata { primary_key: pk, relationships: rels },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::PostgreSql(pool) => {
                match postgres::fetch_postgres_metadata(pool, table).await {
                    Ok((pk, rels)) => DbResponse::Metadata { primary_key: pk, relationships: rels },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Sqlite(pool) => {
                match sqlite::fetch_sqlite_metadata(pool, table).await {
                    Ok((pk, rels)) => DbResponse::Metadata { primary_key: pk, relationships: rels },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            _ => DbResponse::Metadata { primary_key: None, relationships: vec![] },
        }
    }

    async fn handle_load_related_data(
        &self,
        relationship: RelationshipInfo,
        active_row_val: String,
    ) -> DbResponse {
        let escaped_val = active_row_val.replace('\'', "''");
        let query = format!(
            "SELECT * FROM {} WHERE {} = '{}' LIMIT 50;",
            relationship.target_table,
            relationship.target_col,
            escaped_val
        );

        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected".to_string()),
            ActiveConnection::MariaDb(pool) => {
                match mysql::execute_mysql_query(pool, &query).await {
                    Ok((columns, rows)) => DbResponse::RelatedData { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::PostgreSql(pool) => {
                match postgres::execute_postgres_query(pool, &query).await {
                    Ok((columns, rows)) => DbResponse::RelatedData { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Sqlite(pool) => {
                match sqlite::execute_sqlite_query(pool, &query).await {
                    Ok((columns, rows)) => DbResponse::RelatedData { columns, rows },
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            _ => DbResponse::Error("Related data view only supported for relational databases".to_string()),
        }
    }

    async fn trace_select(&self, query: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
        match &self.connection {
            ActiveConnection::MariaDb(pool) => mysql::execute_mysql_query(pool, query).await.map_err(|e| e.to_string()),
            ActiveConnection::PostgreSql(pool) => postgres::execute_postgres_query(pool, query).await.map_err(|e| e.to_string()),
            ActiveConnection::Sqlite(pool) => sqlite::execute_sqlite_query(pool, query).await.map_err(|e| e.to_string()),
            _ => Err("Row trace is only supported for relational databases".to_string()),
        }
    }

    async fn trace_relationships(&self, table: &str) -> Result<Vec<RelationshipInfo>, String> {
        match &self.connection {
            ActiveConnection::MariaDb(pool) => mysql::fetch_mysql_metadata(pool, table).await.map(|(_, r)| r).map_err(|e| e.to_string()),
            ActiveConnection::PostgreSql(pool) => postgres::fetch_postgres_metadata(pool, table).await.map(|(_, r)| r).map_err(|e| e.to_string()),
            ActiveConnection::Sqlite(pool) => sqlite::fetch_sqlite_metadata(pool, table).await.map(|(_, r)| r).map_err(|e| e.to_string()),
            _ => Err("Row trace is only supported for relational databases".to_string()),
        }
    }

    /// Resolve the starting point of a trace from a per-engine condition,
    /// then walk its lineage with the walker matching the active engine.
    async fn handle_trace_start(&self, table: String, condition: String) -> DbResponse {
        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected".to_string()),
            ActiveConnection::MariaDb(_) | ActiveConnection::PostgreSql(_) | ActiveConnection::Sqlite(_) => {
                let query = format!("SELECT * FROM {} WHERE {} LIMIT 1;", table, condition);
                let (columns, rows) = match self.trace_select(&query).await {
                    Ok(res) => res,
                    Err(e) => return DbResponse::Error(e),
                };
                if rows.is_empty() || (columns.len() == 1 && columns[0] == "Status") {
                    return DbResponse::Error(format!("no row in {} matches: {}", table, condition));
                }
                self.handle_trace_row(table, columns, rows.into_iter().next().unwrap()).await
            }
            ActiveConnection::MongoDb { client, database } => {
                trace_mongo(client, database, &table, &condition).await
            }
            ActiveConnection::Neo4j(graph) => trace_neo4j(graph, &table, &condition).await,
            ActiveConnection::LocalJson { .. } => DbResponse::Error(
                "Row trace is not supported for local JSON files (no relations to follow)".to_string(),
            ),
        }
    }

    /// Walk the FK graph in both directions from one row: ancestors
    /// (parents of parents, one row per FK) and descendants (children of
    /// children, a few rows per FK). Bounded by depth, rows-per-relation
    /// and a global node budget; cycles are cut with a visited set.
    async fn handle_trace_row(
        &self,
        table: String,
        columns: Vec<String>,
        values: Vec<String>,
    ) -> DbResponse {
        const MAX_UP: usize = TRACE_MAX_UP;
        const MAX_DOWN: usize = TRACE_MAX_DOWN;
        const ROWS_PER_REL: usize = TRACE_ROWS_PER_REL;
        const NODE_BUDGET: usize = TRACE_NODE_BUDGET;

        if let ActiveConnection::None = self.connection {
            return DbResponse::Error("Not connected".to_string());
        }

        // Arena of nodes + child indices, so the walk stays iterative
        // (async recursion would need boxing) and budget checks are global.
        let mut nodes: Vec<TraceNode> = vec![TraceNode {
            kind: TraceKind::Root,
            table: table.clone(),
            via: String::new(),
            columns,
            values,
            children: Vec::new(),
            note: None,
        }];
        let mut kids: Vec<Vec<usize>> = vec![Vec::new()];
        let mut rel_cache: HashMap<String, Vec<RelationshipInfo>> = HashMap::new();
        // (table, col, value) triples already expanded upward, to cut FK cycles.
        let mut visited_up: HashSet<(String, String, String)> = HashSet::new();

        // Phase 1: ancestors. Phase 2: descendants (root re-queued).
        let mut queue: VecDeque<(usize, usize, bool)> = VecDeque::new();
        queue.push_back((0, 0, true));
        queue.push_back((0, 0, false));
        let mut budget_hit = false;

        while let Some((idx, depth, upward)) = queue.pop_front() {
            let max_depth = if upward { MAX_UP } else { MAX_DOWN };
            if depth >= max_depth {
                nodes[idx].note.get_or_insert_with(|| "depth limit".to_string());
                continue;
            }
            if nodes.len() >= NODE_BUDGET {
                budget_hit = true;
                continue;
            }

            let node_table = nodes[idx].table.clone();
            let rels = match rel_cache.get(&node_table) {
                Some(r) => r.clone(),
                None => match self.trace_relationships(&node_table).await {
                    Ok(r) => {
                        rel_cache.insert(node_table.clone(), r.clone());
                        r
                    }
                    Err(e) => {
                        nodes[idx].note = Some(format!("metadata error: {}", e));
                        continue;
                    }
                },
            };

            for rel in rels.iter().filter(|r| r.is_parent == upward) {
                let col_pos = nodes[idx]
                    .columns
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case(&rel.active_col));
                let val = match col_pos.and_then(|p| nodes[idx].values.get(p)) {
                    Some(v) if v != "NULL" && !v.is_empty() => v.clone(),
                    _ => continue, // FK is null/absent: nothing to follow
                };
                let kind = if upward { TraceKind::Parent } else { TraceKind::Child };
                let via = if upward {
                    format!("{}.{} = {}.{}", node_table, rel.active_col, rel.target_table, rel.target_col)
                } else {
                    format!("{}.{} = {}.{}", rel.target_table, rel.target_col, node_table, rel.active_col)
                };

                if upward {
                    let key = (rel.target_table.clone(), rel.target_col.clone(), val.clone());
                    if !visited_up.insert(key) {
                        nodes.push(TraceNode {
                            kind, table: rel.target_table.clone(), via,
                            columns: vec![], values: vec![],
                            children: vec![], note: Some("cycle: already traced".to_string()),
                        });
                        kids.push(Vec::new());
                        let new_idx = nodes.len() - 1;
                        kids[idx].push(new_idx);
                        continue;
                    }
                }

                let limit = if upward { 1 } else { ROWS_PER_REL + 1 };
                let query = format!(
                    "SELECT * FROM {} WHERE {} = '{}' LIMIT {};",
                    rel.target_table, rel.target_col, val.replace('\'', "''"), limit,
                );
                let (r_cols, r_rows) = match self.trace_select(&query).await {
                    Ok(res) => res,
                    Err(e) => {
                        nodes.push(TraceNode {
                            kind, table: rel.target_table.clone(), via,
                            columns: vec![], values: vec![],
                            children: vec![], note: Some(format!("query error: {}", e)),
                        });
                        kids.push(Vec::new());
                        let new_idx = nodes.len() - 1;
                        kids[idx].push(new_idx);
                        continue;
                    }
                };

                // The engines report an empty result as a "Status" pseudo-row;
                // for a trace that simply means "no related rows here".
                let mut r_rows = r_rows;
                if r_cols.len() == 1 && r_cols[0] == "Status" {
                    r_rows.clear();
                }
                let overflow = !upward && r_rows.len() > ROWS_PER_REL;
                if overflow {
                    r_rows.truncate(ROWS_PER_REL);
                }
                let row_count = r_rows.len();
                for (i, row) in r_rows.into_iter().enumerate() {
                    if nodes.len() >= NODE_BUDGET {
                        budget_hit = true;
                        break;
                    }
                    let note = if overflow && i == row_count - 1 {
                        Some("more rows exist (limit reached)".to_string())
                    } else {
                        None
                    };
                    nodes.push(TraceNode {
                        kind, table: rel.target_table.clone(), via: via.clone(),
                        columns: r_cols.clone(), values: row,
                        children: vec![], note,
                    });
                    kids.push(Vec::new());
                    let new_idx = nodes.len() - 1;
                    kids[idx].push(new_idx);
                    queue.push_back((new_idx, depth + 1, upward));
                }
            }
        }

        if budget_hit {
            nodes[0].note.get_or_insert_with(|| {
                format!("trace truncated at {} nodes", NODE_BUDGET)
            });
        }
        DbResponse::RowTrace(assemble_trace(0, &mut nodes, &kids))
    }

    async fn handle_load_databases(&self) -> DbResponse {
        match &self.connection {
            ActiveConnection::None => DbResponse::Error("Not connected".to_string()),
            ActiveConnection::MariaDb(pool) => {
                match mysql::list_mysql_databases(pool).await {
                    Ok(dbs) => DbResponse::Databases(dbs),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::PostgreSql(pool) => {
                match postgres::list_postgres_databases(pool).await {
                    Ok(dbs) => DbResponse::Databases(dbs),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Sqlite(_) => DbResponse::Databases(vec!["main".to_string()]),
            ActiveConnection::MongoDb { client, .. } => {
                match nosql::list_mongodb_databases(client).await {
                    Ok(dbs) => DbResponse::Databases(dbs),
                    Err(e) => DbResponse::Error(e.to_string()),
                }
            }
            ActiveConnection::Neo4j(_) => DbResponse::Databases(vec!["system".to_string(), "neo4j".to_string()]),
            ActiveConnection::LocalJson { path } => DbResponse::Databases(vec![path.clone()]),
        }
    }

    async fn handle_select_database(&mut self, db_name: String) -> DbResponse {
        let current_config = match &self.active_config {
            Some(cfg) => cfg.clone(),
            None => return DbResponse::Error("Not connected".to_string()),
        };

        let new_config = match current_config {
            DbEngineConfig::MariaDb { url } => DbEngineConfig::MariaDb {
                url: replace_db_in_url(&url, &db_name),
            },
            DbEngineConfig::PostgreSql { url } => DbEngineConfig::PostgreSql {
                url: replace_db_in_url(&url, &db_name),
            },
            DbEngineConfig::Sqlite { path } => DbEngineConfig::Sqlite { path },
            DbEngineConfig::MongoDb { url, .. } => DbEngineConfig::MongoDb {
                url,
                database: db_name,
            },
            DbEngineConfig::Neo4j { url, user, pass } => DbEngineConfig::Neo4j { url, user, pass },
            DbEngineConfig::LocalJson { path } => DbEngineConfig::LocalJson { path },
        };

        let res = self.handle_connect(new_config.clone()).await;
        match res {
            DbResponse::Connected => {
                self.active_config = Some(new_config);
                DbResponse::DatabaseSelected
            }
            other => other,
        }
    }
}

// Shared bounds for all trace walkers.
const TRACE_MAX_UP: usize = 4;       // ancestor depth
const TRACE_MAX_DOWN: usize = 3;     // descendant depth
const TRACE_ROWS_PER_REL: usize = 5; // child rows fetched per relation
const TRACE_NODE_BUDGET: usize = 200;

fn bson_to_display(v: &bson::Bson) -> String {
    match v {
        bson::Bson::Null => "NULL".to_string(),
        bson::Bson::String(s) => s.clone(),
        bson::Bson::Int32(n) => n.to_string(),
        bson::Bson::Int64(n) => n.to_string(),
        bson::Bson::Double(n) => n.to_string(),
        bson::Bson::Boolean(b) => b.to_string(),
        bson::Bson::ObjectId(o) => o.to_hex(),
        bson::Bson::DateTime(d) => d
            .try_to_rfc3339_string()
            .unwrap_or_else(|_| format!("{:?}", d)),
        other => serde_json::to_string(&other.clone().into_relaxed_extjson())
            .unwrap_or_else(|_| format!("{:?}", other)),
    }
}

fn mongo_doc_fields(doc: &bson::Document) -> (Vec<String>, Vec<String>) {
    let mut cols = Vec::new();
    let mut vals = Vec::new();
    for (k, v) in doc.iter() {
        cols.push(k.clone());
        vals.push(bson_to_display(v));
    }
    (cols, vals)
}

/// If a MongoDB field name looks like a reference by naming convention
/// (`user_id`, `id_user`, `userId`), return the base name ("user").
fn mongo_ref_base(field: &str) -> Option<String> {
    let fl = field.to_lowercase();
    if fl == "_id" || fl == "id" {
        return None;
    }
    if let Some(b) = fl.strip_suffix("_id") {
        return Some(b.to_string());
    }
    if let Some(b) = fl.strip_prefix("id_") {
        return Some(b.to_string());
    }
    if field.ends_with("Id") && field.len() > 2 {
        return Some(field[..field.len() - 2].to_lowercase());
    }
    None
}

/// Match a reference base name against the existing collections
/// (exact, plural "s"/"es"), case-insensitively.
fn mongo_find_collection(base: &str, collections: &[String]) -> Option<String> {
    let cands = [base.to_string(), format!("{}s", base), format!("{}es", base)];
    collections
        .iter()
        .find(|c| cands.iter().any(|k| c.eq_ignore_ascii_case(k)))
        .cloned()
}

/// Candidate foreign-key field names that a child collection could use to
/// reference `coll` (e.g. children of "users" may hold user_id / id_user / userId).
fn mongo_child_field_candidates(coll: &str) -> Vec<String> {
    let mut bases = vec![coll.to_lowercase()];
    if let Some(b) = coll.to_lowercase().strip_suffix("es") {
        bases.push(b.to_string());
    }
    if let Some(b) = coll.to_lowercase().strip_suffix('s') {
        bases.push(b.to_string());
    }
    let mut fields = Vec::new();
    for b in bases {
        if b.is_empty() {
            continue;
        }
        fields.push(format!("{}_id", b));
        fields.push(format!("id_{}", b));
        fields.push(format!("{}Id", b));
    }
    fields.sort();
    fields.dedup();
    fields
}

/// Equality variants for a Mongo id value: the value itself, plus the
/// ObjectId/hex-string cross-representations children commonly store.
fn mongo_id_variants(v: &bson::Bson) -> Vec<bson::Bson> {
    let mut out = vec![v.clone()];
    match v {
        bson::Bson::ObjectId(o) => out.push(bson::Bson::String(o.to_hex())),
        bson::Bson::String(s) => {
            if let Ok(oid) = bson::oid::ObjectId::parse_str(s) {
                out.push(bson::Bson::ObjectId(oid));
            }
        }
        _ => {}
    }
    out
}

/// MongoDB lineage walker. Mongo has no declared FKs, so references are
/// inferred by naming convention: fields like `user_id`/`id_user`/`userId`
/// pointing at a collection `user(s)` are treated as parents, and other
/// collections holding such a field equal to this document's _id as children.
async fn trace_mongo(
    client: &mongodb::Client,
    database: &str,
    collection: &str,
    filter_str: &str,
) -> DbResponse {
    let collections = match nosql::list_mongodb_collections(client, database).await {
        Ok(c) => c,
        Err(e) => return DbResponse::Error(e.to_string()),
    };

    let start_filter = match nosql::parse_json_filter(filter_str) {
        Ok(f) => f,
        Err(e) => return DbResponse::Error(e),
    };
    let start_doc = match nosql::find_bson_docs(client, database, collection, start_filter, 1).await {
        Ok(mut docs) if !docs.is_empty() => docs.remove(0),
        Ok(_) => {
            return DbResponse::Error(format!(
                "no document in {} matches: {}",
                collection, filter_str
            ))
        }
        Err(e) => return DbResponse::Error(e),
    };

    let (cols, vals) = mongo_doc_fields(&start_doc);
    let mut nodes: Vec<TraceNode> = vec![TraceNode {
        kind: TraceKind::Root,
        table: collection.to_string(),
        via: String::new(),
        columns: cols,
        values: vals,
        children: Vec::new(),
        note: None,
    }];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new()];
    // Doc backing each arena node, for expansion.
    let mut docs: Vec<(String, bson::Document)> = vec![(collection.to_string(), start_doc)];
    let mut visited: HashSet<String> = HashSet::new();
    if let Some(id) = docs[0].1.get("_id") {
        visited.insert(format!("{}#{:?}", collection, id));
    }

    let mut queue: VecDeque<(usize, usize, bool)> = VecDeque::new();
    queue.push_back((0, 0, true));
    queue.push_back((0, 0, false));
    let mut budget_hit = false;

    while let Some((idx, depth, upward)) = queue.pop_front() {
        let max_depth = if upward { TRACE_MAX_UP } else { TRACE_MAX_DOWN };
        if depth >= max_depth {
            nodes[idx].note.get_or_insert_with(|| "depth limit".to_string());
            continue;
        }
        if nodes.len() >= TRACE_NODE_BUDGET {
            budget_hit = true;
            continue;
        }
        let (node_coll, node_doc) = docs[idx].clone();

        if upward {
            // Parents: reference-looking fields resolved against collections.
            for (field, value) in node_doc.iter() {
                if matches!(value, bson::Bson::Null) {
                    continue;
                }
                let target = match mongo_ref_base(field).and_then(|b| mongo_find_collection(&b, &collections)) {
                    Some(t) => t,
                    None => continue,
                };
                let via = format!("{}.{} → {}._id", node_coll, field, target);
                let or: Vec<bson::Document> = mongo_id_variants(value)
                    .into_iter()
                    .map(|v| bson::doc! { "_id": v })
                    .collect();
                let filter = bson::doc! { "$or": or };
                match nosql::find_bson_docs(client, database, &target, filter, 1).await {
                    Ok(found) => {
                        for doc in found {
                            let key = format!("{}#{:?}", target, doc.get("_id"));
                            let cycle = !visited.insert(key);
                            let (c, v) = mongo_doc_fields(&doc);
                            nodes.push(TraceNode {
                                kind: TraceKind::Parent,
                                table: target.clone(),
                                via: via.clone(),
                                columns: if cycle { vec![] } else { c },
                                values: if cycle { vec![] } else { v },
                                children: vec![],
                                note: cycle.then(|| "cycle: already traced".to_string()),
                            });
                            kids.push(Vec::new());
                            docs.push((target.clone(), doc));
                            let new_idx = nodes.len() - 1;
                            kids[idx].push(new_idx);
                            if !cycle {
                                queue.push_back((new_idx, depth + 1, true));
                            }
                        }
                    }
                    Err(e) => {
                        nodes.push(TraceNode {
                            kind: TraceKind::Parent,
                            table: target.clone(),
                            via,
                            columns: vec![],
                            values: vec![],
                            children: vec![],
                            note: Some(format!("query error: {}", e)),
                        });
                        kids.push(Vec::new());
                        docs.push((target.clone(), bson::Document::new()));
                        let new_idx = nodes.len() - 1;
                        kids[idx].push(new_idx);
                    }
                }
            }
        } else {
            // Children: other collections holding a conventional FK field
            // equal to this document's _id.
            let id = match node_doc.get("_id") {
                Some(v) => v.clone(),
                None => continue,
            };
            let fields = mongo_child_field_candidates(&node_coll);
            for child_coll in collections.iter().filter(|c| *c != &node_coll) {
                if nodes.len() >= TRACE_NODE_BUDGET {
                    budget_hit = true;
                    break;
                }
                let mut or = Vec::new();
                for f in &fields {
                    for v in mongo_id_variants(&id) {
                        or.push(bson::doc! { f.as_str(): v });
                    }
                }
                let filter = bson::doc! { "$or": or };
                let found = match nosql::find_bson_docs(
                    client, database, child_coll, filter, (TRACE_ROWS_PER_REL + 1) as i64,
                ).await {
                    Ok(f) => f,
                    Err(_) => continue, // collection not queryable: skip silently
                };
                let overflow = found.len() > TRACE_ROWS_PER_REL;
                let found: Vec<_> = found.into_iter().take(TRACE_ROWS_PER_REL).collect();
                let count = found.len();
                for (i, doc) in found.into_iter().enumerate() {
                    if nodes.len() >= TRACE_NODE_BUDGET {
                        budget_hit = true;
                        break;
                    }
                    let matched_field = fields
                        .iter()
                        .find(|f| doc.get(f.as_str()).is_some())
                        .cloned()
                        .unwrap_or_else(|| "?".to_string());
                    let via = format!("{}.{} = {}._id", child_coll, matched_field, node_coll);
                    let key = format!("{}#{:?}", child_coll, doc.get("_id"));
                    let cycle = !visited.insert(key);
                    let note = if cycle {
                        Some("cycle: already traced".to_string())
                    } else if overflow && i == count - 1 {
                        Some("more documents exist (limit reached)".to_string())
                    } else {
                        None
                    };
                    let (c, v) = mongo_doc_fields(&doc);
                    nodes.push(TraceNode {
                        kind: TraceKind::Child,
                        table: child_coll.clone(),
                        via,
                        columns: c,
                        values: v,
                        children: vec![],
                        note,
                    });
                    kids.push(Vec::new());
                    docs.push((child_coll.clone(), doc));
                    let new_idx = nodes.len() - 1;
                    kids[idx].push(new_idx);
                    if !cycle {
                        queue.push_back((new_idx, depth + 1, false));
                    }
                }
            }
        }
    }

    if budget_hit {
        nodes[0].note.get_or_insert_with(|| {
            format!("trace truncated at {} nodes", TRACE_NODE_BUDGET)
        });
    }
    DbResponse::RowTrace(assemble_trace(0, &mut nodes, &kids))
}

fn neo4j_node_fields(node: &neo4rs::Node) -> (Vec<String>, Vec<String>) {
    let mut cols = Vec::new();
    let mut vals = Vec::new();
    for k in node.keys() {
        let v = node
            .get::<String>(k)
            .or_else(|_| node.get::<i64>(k).map(|n| n.to_string()))
            .or_else(|_| node.get::<f64>(k).map(|n| n.to_string()))
            .or_else(|_| node.get::<bool>(k).map(|b| b.to_string()))
            .unwrap_or_else(|_| "[complex]".to_string());
        cols.push(k.to_string());
        vals.push(v);
    }
    (cols, vals)
}

fn neo4j_node_label(node: &neo4rs::Node) -> String {
    node.labels().first().map(|s| s.to_string()).unwrap_or_else(|| "Node".to_string())
}

/// Neo4j lineage walker: the graph already has explicit edges, so "parents"
/// are nodes reached by outgoing relationships and "children" by incoming ones.
async fn trace_neo4j(graph: &neo4rs::Graph, label: &str, condition: &str) -> DbResponse {
    // `prop=value` sugar, or a raw Cypher condition if it references `n.`.
    let where_clause = if condition.contains("n.") {
        condition.to_string()
    } else {
        match condition.split_once('=') {
            Some((prop, val)) => {
                let val = val.trim();
                let lit = if val.parse::<f64>().is_ok() || val == "true" || val == "false" {
                    val.to_string()
                } else {
                    format!("'{}'", val.trim_matches(|c| c == '\'' || c == '"').replace('\'', "\\'"))
                };
                format!("n.{} = {}", prop.trim(), lit)
            }
            None => return DbResponse::Error(
                "Neo4j condition must be prop=value or a raw condition using n.".to_string(),
            ),
        }
    };

    let start_cy = format!("MATCH (n:`{}`) WHERE {} RETURN n LIMIT 1", label, where_clause);
    let start = match graph.execute(neo4rs::query(&start_cy)).await {
        Ok(mut stream) => match stream.next().await {
            Ok(Some(row)) => match row.get::<neo4rs::Node>("n") {
                Ok(node) => node,
                Err(e) => return DbResponse::Error(format!("could not read start node: {}", e)),
            },
            Ok(None) => {
                return DbResponse::Error(format!("no node :{} matches: {}", label, condition))
            }
            Err(e) => return DbResponse::Error(e.to_string()),
        },
        Err(e) => return DbResponse::Error(e.to_string()),
    };

    let (cols, vals) = neo4j_node_fields(&start);
    let mut nodes: Vec<TraceNode> = vec![TraceNode {
        kind: TraceKind::Root,
        table: neo4j_node_label(&start),
        via: String::new(),
        columns: cols,
        values: vals,
        children: Vec::new(),
        note: None,
    }];
    let mut kids: Vec<Vec<usize>> = vec![Vec::new()];
    let mut graph_ids: Vec<i64> = vec![start.id()];
    let mut visited: HashSet<i64> = HashSet::new();
    visited.insert(start.id());

    let mut queue: VecDeque<(usize, usize, bool)> = VecDeque::new();
    queue.push_back((0, 0, true));
    queue.push_back((0, 0, false));
    let mut budget_hit = false;

    while let Some((idx, depth, upward)) = queue.pop_front() {
        let max_depth = if upward { TRACE_MAX_UP } else { TRACE_MAX_DOWN };
        if depth >= max_depth {
            nodes[idx].note.get_or_insert_with(|| "depth limit".to_string());
            continue;
        }
        if nodes.len() >= TRACE_NODE_BUDGET {
            budget_hit = true;
            continue;
        }

        let this_id = graph_ids[idx];
        let this_label = nodes[idx].table.clone();
        let cy = if upward {
            format!(
                "MATCH (n)-[r]->(m) WHERE id(n) = {} RETURN type(r) AS rt, m LIMIT {}",
                this_id,
                TRACE_ROWS_PER_REL + 1
            )
        } else {
            format!(
                "MATCH (n)<-[r]-(m) WHERE id(n) = {} RETURN type(r) AS rt, m LIMIT {}",
                this_id,
                TRACE_ROWS_PER_REL + 1
            )
        };

        let mut found: Vec<(String, neo4rs::Node)> = Vec::new();
        match graph.execute(neo4rs::query(&cy)).await {
            Ok(mut stream) => {
                while let Ok(Some(row)) = stream.next().await {
                    let rt = row.get::<String>("rt").unwrap_or_else(|_| "REL".to_string());
                    if let Ok(m) = row.get::<neo4rs::Node>("m") {
                        found.push((rt, m));
                    }
                }
            }
            Err(e) => {
                nodes[idx].note.get_or_insert_with(|| format!("query error: {}", e));
                continue;
            }
        }

        let overflow = found.len() > TRACE_ROWS_PER_REL;
        let found: Vec<_> = found.into_iter().take(TRACE_ROWS_PER_REL).collect();
        let count = found.len();
        for (i, (rel_type, m)) in found.into_iter().enumerate() {
            if nodes.len() >= TRACE_NODE_BUDGET {
                budget_hit = true;
                break;
            }
            let m_label = neo4j_node_label(&m);
            let via = if upward {
                format!("(:{})-[:{}]->(:{})", this_label, rel_type, m_label)
            } else {
                format!("(:{})<-[:{}]-(:{})", this_label, rel_type, m_label)
            };
            let cycle = !visited.insert(m.id());
            let note = if cycle {
                Some("cycle: already traced".to_string())
            } else if overflow && i == count - 1 {
                Some("more nodes exist (limit reached)".to_string())
            } else {
                None
            };
            let (c, v) = neo4j_node_fields(&m);
            nodes.push(TraceNode {
                kind: if upward { TraceKind::Parent } else { TraceKind::Child },
                table: m_label,
                via,
                columns: c,
                values: v,
                children: vec![],
                note,
            });
            kids.push(Vec::new());
            graph_ids.push(m.id());
            let new_idx = nodes.len() - 1;
            kids[idx].push(new_idx);
            if !cycle {
                queue.push_back((new_idx, depth + 1, upward));
            }
        }
    }

    if budget_hit {
        nodes[0].note.get_or_insert_with(|| {
            format!("trace truncated at {} nodes", TRACE_NODE_BUDGET)
        });
    }
    DbResponse::RowTrace(assemble_trace(0, &mut nodes, &kids))
}

/// Rebuild the nested TraceNode tree from the flat arena produced by the walk.
fn assemble_trace(idx: usize, nodes: &mut [TraceNode], kids: &[Vec<usize>]) -> TraceNode {
    let placeholder = TraceNode {
        kind: TraceKind::Root,
        table: String::new(),
        via: String::new(),
        columns: vec![],
        values: vec![],
        children: vec![],
        note: None,
    };
    let mut node = std::mem::replace(&mut nodes[idx], placeholder);
    node.children = kids[idx]
        .iter()
        .map(|&c| assemble_trace(c, nodes, kids))
        .collect();
    node
}

fn replace_db_in_url(url: &str, new_db: &str) -> String {
    let base_url = if let Some(query_idx) = url.find('?') {
        &url[..query_idx]
    } else {
        url
    };
    
    let last_slash_idx = base_url.rfind('/');
    match last_slash_idx {
        Some(idx) => {
            if idx < 9 { 
                format!("{}/{}", url, new_db)
            } else {
                let mut parts = url.splitn(2, '?');
                let path_part = parts.next().unwrap();
                let query_part = parts.next();
                
                let prefix = &path_part[..idx];
                let new_path = format!("{}/{}", prefix, new_db);
                if let Some(q) = query_part {
                    format!("{}?{}", new_path, q)
                } else {
                    new_path
                }
            }
        }
        None => url.to_string(),
    }
}

#[allow(unreachable_patterns)]
fn dbf_value_to_string(value: &dbase::FieldValue) -> String {
    match value {
        dbase::FieldValue::Character(opt_s) => opt_s.as_deref().unwrap_or("").trim().to_string(),
        dbase::FieldValue::Numeric(opt_n) => opt_n.map(|n| n.to_string()).unwrap_or_else(|| "NULL".to_string()),
        dbase::FieldValue::Logical(opt_b) => opt_b.map(|b| if b { "TRUE".to_string() } else { "FALSE".to_string() }).unwrap_or_else(|| "NULL".to_string()),
        dbase::FieldValue::Date(opt_d) => opt_d.map(|d| format!("{:?}", d)).unwrap_or_else(|| "NULL".to_string()),
        dbase::FieldValue::Float(opt_f) => opt_f.map(|f| f.to_string()).unwrap_or_else(|| "NULL".to_string()),
        dbase::FieldValue::Integer(i) => i.to_string(),
        dbase::FieldValue::Currency(c) => c.to_string(),
        dbase::FieldValue::DateTime(dt) => format!("{:?}", dt),
        dbase::FieldValue::Double(d) => d.to_string(),
        dbase::FieldValue::Memo(s) => s.trim().to_string(),
        other => format!("{:?}", other),
    }
}

fn read_local_dbf_file(path: &str) -> Result<DbResponse, String> {
    let mut reader = dbase::Reader::from_path(path)
        .map_err(|e| format!("Failed to open DBF file: {}", e))?;

    let headers: Vec<String> = reader
        .fields()
        .iter()
        .map(|f| f.name().to_string())
        .collect();

    let mut rows = Vec::new();
    for record_result in reader.iter_records() {
        let record = record_result.map_err(|e| format!("Failed to read DBF record: {}", e))?;
        let mut row = Vec::new();
        for header in &headers {
            if let Some(value) = record.get(header) {
                row.push(dbf_value_to_string(value));
            } else {
                row.push("NULL".to_string());
            }
        }
        rows.push(row);
    }

    Ok(DbResponse::QueryResult {
        columns: headers,
        rows,
    })
}
