use crate::db::{DbEngineConfig, DbResponse, RelationshipInfo, DbRequest};
use crate::ui::BiBarData;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveEngine {
    MariaDb,
    PostgreSql,
    Sqlite,
    MongoDb,
    Neo4j,
    LocalJson,
}

impl ActiveEngine {
    pub fn name(&self) -> &'static str {
        match self {
            ActiveEngine::MariaDb => "MariaDB/MySQL",
            ActiveEngine::PostgreSql => "PostgreSQL",
            ActiveEngine::Sqlite => "SQLite",
            ActiveEngine::MongoDb => "MongoDB (NoSQL)",
            ActiveEngine::Neo4j => "Neo4j (Graph)",
            ActiveEngine::LocalJson => "Local File (JSON/BSON/DBF)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationFunction {
    Sum,
    Count,
    Avg,
    Min,
    Max,
    CountDistinct,
    PercentOfRow,
    PercentOfCol,
    PercentOfGrand,
    SumIf,
    CountIf,
    Rate,
    Ratio,
}

impl AggregationFunction {
    pub fn label(&self) -> &'static str {
        match self {
            AggregationFunction::Sum => "SUM",
            AggregationFunction::Count => "COUNT",
            AggregationFunction::Avg => "AVERAGE",
            AggregationFunction::Min => "MIN",
            AggregationFunction::Max => "MAX",
            AggregationFunction::CountDistinct => "DISTINCT COUNT",
            AggregationFunction::PercentOfRow => "% OF ROW TOTAL",
            AggregationFunction::PercentOfCol => "% OF COLUMN TOTAL",
            AggregationFunction::PercentOfGrand => "% OF GRAND TOTAL",
            AggregationFunction::SumIf => "SUM IF (Logical SI)",
            AggregationFunction::CountIf => "COUNT IF (Logical SI)",
            AggregationFunction::Rate => "RATE (Sum/Base Sum * 100)",
            AggregationFunction::Ratio => "RATIO (Sum/Base Sum)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiChartType {
    Bar,
    Sparkline,
    TableOnly,
}

impl BiChartType {
    pub fn label(&self) -> &'static str {
        match self {
            BiChartType::Bar => "Bar Chart",
            BiChartType::Sparkline => "Sparkline Trend",
            BiChartType::TableOnly => "Pivot Table Only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotConfig {
    pub row_dimension_idx: Option<usize>,    // index of column to group as rows
    pub col_dimension_idx: Option<usize>,    // index of column to group as columns
    pub value_column_idx: Option<usize>,     // index of column to aggregate values
    pub agg_fn: AggregationFunction,          // aggregator function
    pub filter_col_idx: Option<usize>,       // column to filter
    pub filter_op: String,                    // filter operator: "=", "!=", ">", "<", "contains"
    pub filter_val: String,                   // filter value string
    pub chart_type: BiChartType,             // visual representation
    pub auto_recalc: bool,                   // auto recalculate
    pub rate_base_column_idx: Option<usize>, // base column index (denominator) for rates
    pub bi_source_related: bool,             // true for parent/child relationship data source
}

#[derive(Clone)]
pub struct BiPivotState {
    pub config: PivotConfig,
    pub active_selector_idx: usize,          // selected config index (0-8)
    pub filter_text_input: String,            // text typing buffer
    pub pivot_headers: Vec<String>,
    pub pivot_rows: Vec<Vec<String>>,
    pub pivot_chart_data: Vec<BiBarData>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    EngineSelector,
    Sidebar,
    SqlConsole,
    QueryResults,
    RelatedDataList,
    RelatedDataGrid,
    ModalEditor,
}

/// Clickable on-screen buttons rendered in the action toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    Search,
    Describe,
    Trace,
    Edit,
    Add,
    Delete,
    Refresh,
    ToggleChart,
    ToggleRelated,
    Databases,
    Back,
    Disconnect,
}

impl ToolbarAction {
    /// (icon+label shown on the button, the keyboard shortcut hint).
    pub fn label(&self) -> &'static str {
        match self {
            ToolbarAction::Search => "⌕ Search",
            ToolbarAction::Describe => "≡ Describe",
            ToolbarAction::Trace => "⛓ Trace",
            ToolbarAction::Edit => "✎ Edit",
            ToolbarAction::Add => "+ Add",
            ToolbarAction::Delete => "✗ Delete",
            ToolbarAction::Refresh => "⟲ Refresh",
            ToolbarAction::ToggleChart => "▤ Chart",
            ToolbarAction::ToggleRelated => "⮌ Related",
            ToolbarAction::Databases => "⛁ DBs",
            ToolbarAction::Back => "← Back",
            ToolbarAction::Disconnect => "✕ Disconnect",
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            ToolbarAction::Search => "/",
            ToolbarAction::Describe => "i",
            ToolbarAction::Trace => "t",
            ToolbarAction::Edit => "e",
            ToolbarAction::Add => "a",
            ToolbarAction::Delete => "d",
            ToolbarAction::Refresh => "r",
            ToolbarAction::ToggleChart => "F6",
            ToolbarAction::ToggleRelated => "F7",
            ToolbarAction::Databases => "D",
            ToolbarAction::Back => "⌫",
            ToolbarAction::Disconnect => "Esc",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Profiles,
    Form,
    RawUrl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormField {
    Host,
    Port,
    User,
    Pass,
    Db,
}

impl FormField {
    pub fn label(&self, engine: ActiveEngine) -> &'static str {
        match self {
            FormField::Host => "Host/Server IP:",
            FormField::Port => "Port:",
            FormField::User => "Username:",
            FormField::Pass => "Password:",
            FormField::Db => match engine {
                ActiveEngine::Sqlite => "Database File Path:",
                ActiveEngine::LocalJson => "JSON/BSON File Path:",
                ActiveEngine::MongoDb => "MongoDB Database Name (Optional):",
                _ => "Database Name (Optional):",
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionProfile {
    pub name: String,
    pub engine: ActiveEngine,
    pub config: DbEngineConfig,
}

pub struct ConnectionForm {
    pub host: String,
    pub port: String,
    pub user: String,
    pub pass: String,
    pub db_or_path: String,
}

pub struct ConnectionFields {
    pub mysql_url: String,
    pub postgres_url: String,
    pub sqlite_path: String,
    pub mongodb_url: String,
    pub mongodb_db: String,
    pub neo4j_url: String,
    pub neo4j_user: String,
    pub neo4j_pass: String,
    pub json_path: String,
}

pub struct TreeItem {
    pub key: String,
    pub value_summary: String,
    pub depth: usize,
    pub is_expanded: bool,
    pub children: Vec<TreeItem>,
    pub path: Vec<String>,
}

pub struct FlatTreeRow {
    pub display_text: String,
    pub path: Vec<String>,
    pub depth: usize,
}

impl TreeItem {
    pub fn flatten(&self, list: &mut Vec<FlatTreeRow>) {
        let prefix = if self.children.is_empty() {
            "▪"
        } else if self.is_expanded {
            "▼"
        } else {
            "▶"
        };
        
        list.push(FlatTreeRow {
            display_text: format!(
                "{:indent$}{} {}: {}",
                "",
                prefix,
                self.key,
                self.value_summary,
                indent = self.depth * 2
            ),
            path: self.path.clone(),
            depth: self.depth,
        });

        if self.is_expanded {
            for child in &self.children {
                child.flatten(list);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExplorationState {
    pub table_name: Option<String>,
    pub query: String,
    pub selected_row_idx: Option<usize>,
    pub active_relationship_idx: usize,
    pub show_related_split: bool,
}

pub struct AppState {
    pub should_quit: bool,
    pub active_pane: ActivePane,
    pub active_engine: ActiveEngine,
    
    // Connection configs
    pub conn_fields: ConnectionFields,
    pub connection_mode: ConnectionMode,
    pub profiles: Vec<ConnectionProfile>,
    pub selected_profile_idx: usize,
    pub form_fields: ConnectionForm,
    pub active_form_field: FormField,

    pub connected: bool,
    pub connecting: bool,
    pub conn_status_msg: String,

    // Sidebar tables/collections
    pub tables: Vec<String>,
    pub selected_table_idx: Option<usize>,
    pub active_table_name: Option<String>,

    // Sidebar databases/schemas list
    pub databases: Vec<String>,
    pub selected_db_idx: Option<usize>,
    pub active_db_name: Option<String>,
    pub show_db_list: bool,

    // Console
    pub sql_console_input: String,
    pub sql_cursor_pos: usize,

    // Relational/Graph Results
    pub result_headers: Vec<String>,
    pub result_rows: Vec<Vec<String>>,
    pub selected_row_idx: Option<usize>,

    // Document Results (MongoDB/JSON)
    pub tree_roots: Vec<TreeItem>,
    pub flat_tree_rows: Vec<FlatTreeRow>,
    pub selected_tree_row_idx: Option<usize>,

    // BI Visualization
    pub bi_mode_enabled: bool,
    pub bi_chartable: bool,
    pub bi_bar_data: Vec<BiBarData>,
    pub bi_spark_data: Vec<u64>,
    pub pivot_state: BiPivotState,

    // Related Data / Parent-Child Joins Inspector
    pub show_related_split: bool,
    pub primary_key: Option<String>,
    pub relationships: Vec<RelationshipInfo>,
    pub active_relationship_idx: usize,
    pub related_headers: Vec<String>,
    pub related_rows: Vec<Vec<String>>,
    pub related_selected_row_idx: Option<usize>,
    pub related_loading: bool,
    pub exploration_history: Vec<ExplorationState>,

    // Searcher / Query bar / Describe (connector UX)
    pub search_active: bool,           // true while typing in the in-grid searcher
    pub search_query: String,          // current filter text (client-side substring match)
    pub last_executed_query: String,   // query that produced the data currently shown
    pub show_describe: bool,           // describe/schema overlay visible
    pub describe_headers: Vec<String>, // describe table headers
    pub describe_rows: Vec<Vec<String>>, // describe table rows

    // Sidebar quick-find: one filter box shared by the tables list and the
    // databases list (whichever is currently shown).
    pub sidebar_filter: String,
    pub sidebar_filter_active: bool,

    // lazysql-style grid navigation
    pub selected_col_idx: usize,     // cell cursor column (index into result_headers)
    pub show_row_detail: bool,       // record view: selected row as column/value list
    pub row_detail_scroll: usize,
    pub row_detail_line_count: usize, // set by the UI each frame, used to clamp scroll

    // Row trace overlay (full lineage of the selected row)
    pub show_trace: bool,
    pub trace_loading: bool,
    pub trace_root: Option<crate::db::TraceNode>,
    pub trace_error: Option<String>,
    pub trace_json_mode: bool, // false = tree view, true = raw JSON view
    pub trace_scroll: usize,
    pub trace_line_count: usize, // set by the UI each frame, used to clamp scroll

    // Interactive Modal Row Editors
    pub show_edit_modal: bool,
    pub show_add_modal: bool,
    pub show_delete_confirm: bool,
    pub modal_fields: Vec<(String, String)>,
    pub active_modal_field_idx: usize,
    pub selected_row_pk_val: Option<String>,

    // Spinner/Tick count for loader
    pub tick_count: usize,
    pub mutation_in_progress: Option<String>,

    pub col_scroll_offset: usize,
    pub related_col_scroll_offset: usize,

    // Layout Rects for mouse support
    pub rect_sidebar: Option<Rect>,
    pub rect_data_view: Option<Rect>,
    pub rect_sql_console: Option<Rect>,
    pub rect_header_tabs: Option<Rect>,
    pub rect_bi_config: Option<Rect>,
    pub rect_bi_pivot: Option<Rect>,
    pub rect_related_split: Option<Rect>,
    pub rect_related_list: Option<Rect>,
    pub rect_related_grid: Option<Rect>,
    pub rect_query_bar: Option<Rect>,
    pub rect_describe: Option<Rect>,
    pub rect_trace: Option<Rect>,
    pub rect_row_detail: Option<Rect>,
    // Clickable toolbar buttons: rebuilt every frame by the UI.
    pub toolbar_buttons: Vec<(Rect, ToolbarAction)>,
}

impl AppState {
    pub fn new() -> Self {
        // Auto-discover credentials
        let mysql_url = if let Some(cfg) = crate::db::config::MySqlConfig::from_my_cnf() {
            cfg.to_connection_string("INOPCONBD")
        } else {
            "mysql://root@127.0.0.1:3306/INOPCONBD".to_string()
        };

        let postgres_url = if let Some(cfg) = crate::db::config::PgCredentials::from_pgpass("127.0.0.1", 5432, "postgres", "postgres") {
            cfg.to_connection_string()
        } else {
            "postgres://postgres@localhost/postgres".to_string()
        };

        let mut app = Self {
            should_quit: false,
            active_pane: ActivePane::EngineSelector,
            active_engine: ActiveEngine::MariaDb,
            conn_fields: ConnectionFields {
                mysql_url,
                postgres_url,
                sqlite_path: "/home/davidpa/SaasActivos.API/database.sqlite".to_string(),
                mongodb_url: "mongodb://localhost:27017".to_string(),
                mongodb_db: "local".to_string(),
                neo4j_url: "bolt://localhost:7687".to_string(),
                neo4j_user: "neo4j".to_string(),
                neo4j_pass: "password".to_string(),
                json_path: "/home/davidpa/recetas_schema.json".to_string(),
            },
            connection_mode: ConnectionMode::Profiles,
            profiles: vec![],
            selected_profile_idx: 0,
            form_fields: ConnectionForm {
                host: "127.0.0.1".to_string(),
                port: "3306".to_string(),
                user: "root".to_string(),
                pass: "".to_string(),
                db_or_path: "INOPCONBD".to_string(),
            },
            active_form_field: FormField::Host,
            connected: false,
            connecting: false,
            conn_status_msg: "Not connected".to_string(),
            tables: vec![],
            selected_table_idx: None,
            active_table_name: None,
            databases: vec![],
            selected_db_idx: None,
            active_db_name: None,
            show_db_list: false,
            sql_console_input: String::new(),
            sql_cursor_pos: 0,
            result_headers: vec![],
            result_rows: vec![],
            selected_row_idx: None,
            tree_roots: vec![],
            flat_tree_rows: vec![],
            selected_tree_row_idx: None,
            bi_mode_enabled: false,
            bi_chartable: false,
            bi_bar_data: vec![],
            bi_spark_data: vec![],
            pivot_state: BiPivotState {
                config: PivotConfig {
                    row_dimension_idx: None,
                    col_dimension_idx: None,
                    value_column_idx: None,
                    agg_fn: AggregationFunction::Count,
                    filter_col_idx: None,
                    filter_op: "=".to_string(),
                    filter_val: "".to_string(),
                    chart_type: BiChartType::TableOnly,
                    auto_recalc: true,
                    rate_base_column_idx: None,
                    bi_source_related: false,
                },
                active_selector_idx: 0,
                filter_text_input: "".to_string(),
                pivot_headers: vec![],
                pivot_rows: vec![],
                pivot_chart_data: vec![],
            },
            show_related_split: true,
            primary_key: None,
            relationships: vec![],
            active_relationship_idx: 0,
            related_headers: vec![],
            related_rows: vec![],
            related_selected_row_idx: None,
            related_loading: false,
            exploration_history: vec![],
            search_active: false,
            search_query: String::new(),
            last_executed_query: String::new(),
            show_describe: false,
            describe_headers: vec![],
            describe_rows: vec![],
            sidebar_filter: String::new(),
            sidebar_filter_active: false,
            selected_col_idx: 0,
            show_row_detail: false,
            row_detail_scroll: 0,
            row_detail_line_count: 0,
            show_trace: false,
            trace_loading: false,
            trace_root: None,
            trace_error: None,
            trace_json_mode: false,
            trace_scroll: 0,
            trace_line_count: 0,
            show_edit_modal: false,
            show_add_modal: false,
            show_delete_confirm: false,
            modal_fields: vec![],
            active_modal_field_idx: 0,
            selected_row_pk_val: None,
            tick_count: 0,
            mutation_in_progress: None,
            col_scroll_offset: 0,
            related_col_scroll_offset: 0,
            rect_sidebar: None,
            rect_data_view: None,
            rect_sql_console: None,
            rect_header_tabs: None,
            rect_bi_config: None,
            rect_bi_pivot: None,
            rect_related_split: None,
            rect_related_list: None,
            rect_related_grid: None,
            rect_query_bar: None,
            rect_describe: None,
            rect_trace: None,
            rect_row_detail: None,
            toolbar_buttons: vec![],
        };

        app.load_discovered_profiles();
        app.sync_form_fields_for_engine();
        app.conn_status_msg = format!("Ready to connect to {}", app.active_engine.name());
        app
    }

    pub fn active_connection_config(&self) -> DbEngineConfig {
        match self.connection_mode {
            ConnectionMode::Profiles => {
                if self.selected_profile_idx < self.profiles.len() {
                    self.profiles[self.selected_profile_idx].config.clone()
                } else {
                    self.get_form_connection_config()
                }
            }
            ConnectionMode::Form => self.get_form_connection_config(),
            ConnectionMode::RawUrl => match self.active_engine {
                ActiveEngine::MariaDb => DbEngineConfig::MariaDb {
                    url: self.conn_fields.mysql_url.clone(),
                },
                ActiveEngine::PostgreSql => DbEngineConfig::PostgreSql {
                    url: self.conn_fields.postgres_url.clone(),
                },
                ActiveEngine::Sqlite => DbEngineConfig::Sqlite {
                    path: self.conn_fields.sqlite_path.clone(),
                },
                ActiveEngine::MongoDb => DbEngineConfig::MongoDb {
                    url: self.conn_fields.mongodb_url.clone(),
                    database: self.conn_fields.mongodb_db.clone(),
                },
                ActiveEngine::Neo4j => DbEngineConfig::Neo4j {
                    url: self.conn_fields.neo4j_url.clone(),
                    user: self.conn_fields.neo4j_user.clone(),
                    pass: self.conn_fields.neo4j_pass.clone(),
                },
                ActiveEngine::LocalJson => DbEngineConfig::LocalJson {
                    path: self.conn_fields.json_path.clone(),
                },
            },
        }
    }

    pub fn load_discovered_profiles(&mut self) {
        self.profiles.clear();

        if let Some(cfg) = crate::db::config::MySqlConfig::from_my_cnf() {
            self.profiles.push(ConnectionProfile {
                name: format!("MySQL (cnf) - {}@{}:{}", cfg.user, cfg.host, cfg.port),
                engine: ActiveEngine::MariaDb,
                config: DbEngineConfig::MariaDb {
                    url: cfg.to_connection_string("INOPCONBD"),
                },
            });
        }

        let home = std::env::var("HOME").unwrap_or_default();
        let pgpass_path = Path::new(&home).join(".pgpass");
        if pgpass_path.exists() {
            if let Ok(file) = File::open(pgpass_path) {
                let reader = BufReader::new(file);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.starts_with('#') || trimmed.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = trimmed.split(':').collect();
                    if parts.len() == 5 {
                        let host = parts[0];
                        let port = parts[1];
                        let db = parts[2];
                        let user = parts[3];
                        let pass = parts[4];
                        let db_name = if db == "*" { "postgres" } else { db };
                        let pg_url = format!("postgres://{}:{}@{}:{}/{}", user, pass, host, port, db_name);
                        self.profiles.push(ConnectionProfile {
                            name: format!("Postgres (pgpass) - {}@{}:{}/{}", user, host, port, db_name),
                            engine: ActiveEngine::PostgreSql,
                            config: DbEngineConfig::PostgreSql { url: pg_url },
                        });
                    }
                }
            }
        }

        self.profiles.push(ConnectionProfile {
            name: "PostgreSQL (Default Local)".to_string(),
            engine: ActiveEngine::PostgreSql,
            config: DbEngineConfig::PostgreSql { url: "postgres://postgres@localhost/postgres".to_string() },
        });
        self.profiles.push(ConnectionProfile {
            name: "MariaDB/MySQL (Default Local)".to_string(),
            engine: ActiveEngine::MariaDb,
            config: DbEngineConfig::MariaDb { url: "mysql://root@127.0.0.1:3306/INOPCONBD".to_string() },
        });
        self.profiles.push(ConnectionProfile {
            name: "SQLite (SaasActivos database.sqlite)".to_string(),
            engine: ActiveEngine::Sqlite,
            config: DbEngineConfig::Sqlite { path: self.conn_fields.sqlite_path.clone() },
        });
        self.profiles.push(ConnectionProfile {
            name: "MongoDB (Default Local)".to_string(),
            engine: ActiveEngine::MongoDb,
            config: DbEngineConfig::MongoDb {
                url: self.conn_fields.mongodb_url.clone(),
                database: self.conn_fields.mongodb_db.clone(),
            },
        });
        self.profiles.push(ConnectionProfile {
            name: "Neo4j (Default Local)".to_string(),
            engine: ActiveEngine::Neo4j,
            config: DbEngineConfig::Neo4j {
                url: self.conn_fields.neo4j_url.clone(),
                user: self.conn_fields.neo4j_user.clone(),
                pass: self.conn_fields.neo4j_pass.clone(),
            },
        });
        self.profiles.push(ConnectionProfile {
            name: "JSON (Recetas Schema)".to_string(),
            engine: ActiveEngine::LocalJson,
            config: DbEngineConfig::LocalJson { path: self.conn_fields.json_path.clone() },
        });
    }

    pub fn sync_form_fields_for_engine(&mut self) {
        match self.active_engine {
            ActiveEngine::MariaDb => {
                self.form_fields.host = "127.0.0.1".to_string();
                self.form_fields.port = "3306".to_string();
                self.form_fields.user = "root".to_string();
                self.form_fields.pass = "".to_string();
                // Leave blank = connect to the server, then pick a DB with 'd'.
                self.form_fields.db_or_path = String::new();
            }
            ActiveEngine::PostgreSql => {
                self.form_fields.host = "127.0.0.1".to_string();
                self.form_fields.port = "5432".to_string();
                self.form_fields.user = "postgres".to_string();
                self.form_fields.pass = "".to_string();
                self.form_fields.db_or_path = "postgres".to_string();
            }
            ActiveEngine::Sqlite => {
                self.form_fields.host = "".to_string();
                self.form_fields.port = "".to_string();
                self.form_fields.user = "".to_string();
                self.form_fields.pass = "".to_string();
                self.form_fields.db_or_path = self.conn_fields.sqlite_path.clone();
            }
            ActiveEngine::MongoDb => {
                self.form_fields.host = "127.0.0.1".to_string();
                self.form_fields.port = "27017".to_string();
                self.form_fields.user = "".to_string();
                self.form_fields.pass = "".to_string();
                self.form_fields.db_or_path = "local".to_string();
            }
            ActiveEngine::Neo4j => {
                self.form_fields.host = "127.0.0.1".to_string();
                self.form_fields.port = "7687".to_string();
                self.form_fields.user = "neo4j".to_string();
                self.form_fields.pass = "password".to_string();
                self.form_fields.db_or_path = "".to_string();
            }
            ActiveEngine::LocalJson => {
                self.form_fields.host = "".to_string();
                self.form_fields.port = "".to_string();
                self.form_fields.user = "".to_string();
                self.form_fields.pass = "".to_string();
                self.form_fields.db_or_path = self.conn_fields.json_path.clone();
            }
        }
    }

    pub fn get_form_connection_config(&self) -> DbEngineConfig {
        let host = self.form_fields.host.trim();
        let port = self.form_fields.port.trim();
        let user = self.form_fields.user.trim();
        let pass = &self.form_fields.pass;
        // Treat the old "-----" placeholder (and whitespace) as "no database given".
        let db_raw = self.form_fields.db_or_path.trim();
        let db = if db_raw.chars().all(|c| c == '-') { "" } else { db_raw };

        match self.active_engine {
            ActiveEngine::MariaDb => {
                let credentials = if pass.is_empty() {
                    user.to_string()
                } else {
                    format!("{}:{}", user, pass)
                };
                // No DB given -> connect to the server (no /db); pick one with 'd'.
                let url = if db.is_empty() {
                    format!("mysql://{}@{}:{}", credentials, host, port)
                } else {
                    format!("mysql://{}@{}:{}/{}", credentials, host, port, db)
                };
                DbEngineConfig::MariaDb { url }
            }
            ActiveEngine::PostgreSql => {
                let credentials = if pass.is_empty() {
                    user.to_string()
                } else {
                    format!("{}:{}", user, pass)
                };
                let db_part = if db.is_empty() { "postgres" } else { db };
                DbEngineConfig::PostgreSql {
                    url: format!("postgres://{}@{}:{}/{}", credentials, host, port, db_part),
                }
            }
            ActiveEngine::Sqlite => DbEngineConfig::Sqlite {
                path: db_raw.to_string(),
            },
            ActiveEngine::MongoDb => {
                let url = if user.is_empty() {
                    format!("mongodb://{}:{}", host, port)
                } else {
                    format!("mongodb://{}:{}@{}:{}", user, pass, host, port)
                };
                let db_part = if db.is_empty() { "admin" } else { db };
                DbEngineConfig::MongoDb {
                    url,
                    database: db_part.to_string(),
                }
            }
            ActiveEngine::Neo4j => {
                let url = format!("bolt://{}:{}", host, port);
                DbEngineConfig::Neo4j {
                    url,
                    user: user.to_string(),
                    pass: pass.clone(),
                }
            }
            ActiveEngine::LocalJson => DbEngineConfig::LocalJson {
                path: db_raw.to_string(),
            },
        }
    }

    pub fn cycle_focus(&mut self) {
        if !self.connected {
            self.active_pane = ActivePane::EngineSelector;
            return;
        }
        self.active_pane = match self.active_pane {
            ActivePane::EngineSelector => ActivePane::Sidebar,
            ActivePane::Sidebar => ActivePane::SqlConsole,
            ActivePane::SqlConsole => ActivePane::QueryResults,
            ActivePane::QueryResults => {
                if self.show_related_split && !self.relationships.is_empty() {
                    ActivePane::RelatedDataList
                } else {
                    ActivePane::Sidebar
                }
            }
            ActivePane::RelatedDataList => ActivePane::RelatedDataGrid,
            ActivePane::RelatedDataGrid => ActivePane::Sidebar,
            ActivePane::ModalEditor => ActivePane::ModalEditor, // locked in modal
        };
    }

    pub fn cycle_focus_back(&mut self) {
        if !self.connected {
            self.active_pane = ActivePane::EngineSelector;
            return;
        }
        self.active_pane = match self.active_pane {
            ActivePane::EngineSelector => ActivePane::RelatedDataGrid,
            ActivePane::Sidebar => {
                if self.show_related_split && !self.relationships.is_empty() {
                    ActivePane::RelatedDataGrid
                } else {
                    ActivePane::QueryResults
                }
            }
            ActivePane::SqlConsole => ActivePane::Sidebar,
            ActivePane::QueryResults => ActivePane::SqlConsole,
            ActivePane::RelatedDataList => ActivePane::QueryResults,
            ActivePane::RelatedDataGrid => ActivePane::RelatedDataList,
            ActivePane::ModalEditor => ActivePane::ModalEditor,
        };
    }

    pub fn select_next_engine(&mut self) {
        if self.active_pane == ActivePane::EngineSelector && !self.connected {
            self.active_engine = match self.active_engine {
                ActiveEngine::MariaDb => ActiveEngine::PostgreSql,
                ActiveEngine::PostgreSql => ActiveEngine::Sqlite,
                ActiveEngine::Sqlite => ActiveEngine::MongoDb,
                ActiveEngine::MongoDb => ActiveEngine::Neo4j,
                ActiveEngine::Neo4j => ActiveEngine::LocalJson,
                ActiveEngine::LocalJson => ActiveEngine::MariaDb,
            };
            self.sync_form_fields_for_engine();
            self.conn_status_msg = format!("Ready to connect to {}", self.active_engine.name());
        }
    }

    pub fn select_prev_engine(&mut self) {
        if self.active_pane == ActivePane::EngineSelector && !self.connected {
            self.active_engine = match self.active_engine {
                ActiveEngine::MariaDb => ActiveEngine::LocalJson,
                ActiveEngine::PostgreSql => ActiveEngine::MariaDb,
                ActiveEngine::Sqlite => ActiveEngine::PostgreSql,
                ActiveEngine::MongoDb => ActiveEngine::Sqlite,
                ActiveEngine::Neo4j => ActiveEngine::MongoDb,
                ActiveEngine::LocalJson => ActiveEngine::Neo4j,
            };
            self.sync_form_fields_for_engine();
            self.conn_status_msg = format!("Ready to connect to {}", self.active_engine.name());
        }
    }

    pub fn set_connected(&mut self) {
        self.connected = true;
        self.connecting = false;
        self.conn_status_msg = format!("Connected to {}", self.active_engine.name());
        self.sql_console_input = match self.active_engine {
            ActiveEngine::MariaDb | ActiveEngine::PostgreSql | ActiveEngine::Sqlite => {
                "SELECT * FROM LIMIT 10;".to_string()
            }
            ActiveEngine::MongoDb => "collection_name|{}".to_string(),
            ActiveEngine::Neo4j => "MATCH (n) RETURN n LIMIT 10;".to_string(),
            ActiveEngine::LocalJson => "".to_string(),
        };
        self.sql_cursor_pos = self.sql_console_input.len();
        self.active_pane = ActivePane::Sidebar;
    }

    pub fn set_response(&mut self, res: DbResponse) {
        self.connecting = false;
        self.related_loading = false;
        match res {
            DbResponse::Connected => {
                self.set_connected();
            }
            DbResponse::Databases(dbs) => {
                self.databases = dbs;
                self.selected_db_idx = if !self.databases.is_empty() { Some(0) } else { None };
                self.sidebar_filter.clear();
                self.sidebar_filter_active = false;
            }
            DbResponse::DatabaseSelected => {
                self.tables.clear();
                self.selected_table_idx = None;
                self.active_table_name = None;
                self.exploration_history.clear();
            }
            DbResponse::Tables(tbls) => {
                self.tables = tbls;
                self.exploration_history.clear();
                self.sidebar_filter.clear();
                self.sidebar_filter_active = false;
                if !self.tables.is_empty() {
                    self.selected_table_idx = Some(0);
                    let table = &self.tables[0];
                    self.active_table_name = Some(table.clone());
                    self.sql_console_input = match self.active_engine {
                        ActiveEngine::MariaDb | ActiveEngine::PostgreSql | ActiveEngine::Sqlite => {
                            format!("SELECT * FROM {} LIMIT 50;", table)
                        }
                        ActiveEngine::MongoDb => {
                            format!("{}|{{}}", table)
                        }
                        ActiveEngine::Neo4j => {
                            format!("MATCH (n:{}) RETURN n LIMIT 25;", table)
                        }
                        ActiveEngine::LocalJson => "".to_string(),
                    };
                    self.sql_cursor_pos = self.sql_console_input.len();
                } else {
                    self.selected_table_idx = None;
                    self.active_table_name = None;
                }
            }
            DbResponse::QueryResult { columns, rows } => {
                self.result_headers = columns;
                self.result_rows = rows;
                self.selected_row_idx = if !self.result_rows.is_empty() { Some(0) } else { None };
                self.bi_mode_enabled = false;
                self.last_executed_query = self.sql_console_input.clone();
                self.search_active = false;
                self.search_query.clear();
                self.col_scroll_offset = 0;
                self.selected_col_idx = 0;
                self.show_row_detail = false;
                self.row_detail_scroll = 0;
                self.rebuild_describe();

                let cols_ref = self.result_headers.clone();
                let rows_ref = self.result_rows.clone();
                self.parse_bi_data(&cols_ref, &rows_ref);
                
                // Clear old relationship data only if we don't have an active table
                if self.active_table_name.is_none() {
                    self.primary_key = None;
                    self.relationships.clear();
                }
                self.related_headers.clear();
                self.related_rows.clear();
                self.related_col_scroll_offset = 0;
            }
            DbResponse::Metadata { primary_key, relationships } => {
                self.primary_key = primary_key;
                self.relationships = relationships;
                self.active_relationship_idx = 0;
                self.related_col_scroll_offset = 0;
                self.rebuild_describe();
            }
            DbResponse::RelatedData { columns, rows } => {
                self.related_headers = columns;
                self.related_rows = rows;
                self.related_selected_row_idx = if !self.related_rows.is_empty() { Some(0) } else { None };
                self.related_col_scroll_offset = 0;
            }
            DbResponse::RowTrace(root) => {
                self.trace_loading = false;
                self.trace_error = None;
                self.trace_root = Some(root);
                self.trace_scroll = 0;
            }
            DbResponse::DocumentResult(docs) => {
                self.tree_roots.clear();
                for (i, doc) in docs.iter().enumerate() {
                    let root_key = format!("Document {}", i + 1);
                    let tree_root = json_to_tree_item(&root_key, doc, 0, vec![]);
                    self.tree_roots.push(tree_root);
                }
                self.update_flat_tree();
                self.selected_tree_row_idx = if !self.flat_tree_rows.is_empty() { Some(0) } else { None };
                self.last_executed_query = self.sql_console_input.clone();
                self.search_active = false;
                self.search_query.clear();
            }
            DbResponse::Error(err) => {
                if self.show_trace && self.trace_loading {
                    // Keep the results grid intact: show the error inside the overlay.
                    self.trace_loading = false;
                    self.trace_error = Some(err);
                } else {
                    self.conn_status_msg = format!("Error: {}", err);
                    self.result_headers = vec!["Error".to_string()];
                    self.result_rows = vec![vec![err]];
                    self.selected_row_idx = Some(0);
                }
            }
        }
    }

    pub fn update_flat_tree(&mut self) {
        self.flat_tree_rows.clear();
        for root in &self.tree_roots {
            root.flatten(&mut self.flat_tree_rows);
        }
    }

    pub fn toggle_selected_tree_item(&mut self) {
        if let Some(idx) = self.selected_tree_row_idx {
            if idx < self.flat_tree_rows.len() {
                let target_path = &self.flat_tree_rows[idx].path;
                let mut roots = std::mem::take(&mut self.tree_roots);
                let _ = toggle_tree_node(&mut roots, target_path);
                self.tree_roots = roots;
                self.update_flat_tree();
            }
        }
    }

    pub fn parse_bi_data(&mut self, columns: &[String], rows: &[Vec<String>]) {
        self.bi_bar_data.clear();
        self.bi_spark_data.clear();
        self.bi_chartable = false;

        if columns.is_empty() || rows.is_empty() {
            return;
        }

        // 1. Try to find a numeric column and a label column
        let mut numeric_col_idx = None;
        let mut label_col_idx = None;

        let sample_limit = rows.len().min(15);
        for c_idx in 0..columns.len() {
            let mut numeric_count = 0;
            for row in rows.iter().take(sample_limit) {
                if let Some(val) = row.get(c_idx) {
                    if val.parse::<f64>().is_ok() {
                        numeric_count += 1;
                    }
                }
            }
            if numeric_count > sample_limit / 2 {
                if numeric_col_idx.is_none() {
                    numeric_col_idx = Some(c_idx);
                }
            } else if label_col_idx.is_none() {
                label_col_idx = Some(c_idx);
            }
        }

        if let Some(num_idx) = numeric_col_idx {
            let lbl_idx = label_col_idx.unwrap_or(0);
            for row in rows {
                let label = row.get(lbl_idx).cloned().unwrap_or_else(|| "Index".to_string());
                let val_str = row.get(num_idx).map(|x| x.as_str()).unwrap_or("0");
                let num_val = val_str.parse::<f64>().unwrap_or(0.0).round() as u64;
                self.bi_bar_data.push(BiBarData {
                    label,
                    value: num_val,
                });
                self.bi_spark_data.push(num_val);
            }
            self.bi_chartable = true;
        } else {
            // Fallback frequency histogram of the first column
            let mut frequency_map = std::collections::HashMap::new();
            for row in rows {
                if let Some(val) = row.get(0) {
                    *frequency_map.entry(val.clone()).or_insert(0u64) += 1;
                }
            }
            
            let mut sorted_counts: Vec<(String, u64)> = frequency_map.into_iter().collect();
            sorted_counts.sort_by(|a, b| b.1.cmp(&a.1));
            
            for (lbl, count) in sorted_counts.into_iter().take(10) {
                self.bi_bar_data.push(BiBarData {
                    label: lbl,
                    value: count,
                });
                self.bi_spark_data.push(count);
            }
            self.bi_chartable = true;
        }
    }

    pub fn recalculate_pivot(&mut self) {
        let (cols, rows) = if self.pivot_state.config.bi_source_related {
            (&self.related_headers, &self.related_rows)
        } else {
            (&self.result_headers, &self.result_rows)
        };
        
        self.pivot_state.pivot_headers.clear();
        self.pivot_state.pivot_rows.clear();
        self.pivot_state.pivot_chart_data.clear();
        
        if cols.is_empty() || rows.is_empty() {
            return;
        }
        
        // 1. Filter rows (global filters apply except for local conditional aggregations)
        let mut filtered_rows = Vec::new();
        let agg_fn = self.pivot_state.config.agg_fn;
        let is_conditional_agg = matches!(agg_fn, AggregationFunction::SumIf | AggregationFunction::CountIf);
        
        if let Some(f_idx) = self.pivot_state.config.filter_col_idx {
            if is_conditional_agg {
                // For conditional aggregations, we do NOT filter the rows globally,
                // so we can evaluate the condition per-cell/per-row during aggregation.
                filtered_rows = rows.clone();
            } else {
                let op = &self.pivot_state.config.filter_op;
                let val = &self.pivot_state.config.filter_val;
                
                for row in rows {
                    if let Some(cell) = row.get(f_idx) {
                        let mut matched = false;
                        if op == "=" {
                            matched = cell.to_lowercase() == val.to_lowercase();
                        } else if op == "!=" {
                            matched = cell.to_lowercase() != val.to_lowercase();
                        } else if op == ">" {
                            if let (Ok(c_num), Ok(v_num)) = (cell.parse::<f64>(), val.parse::<f64>()) {
                                matched = c_num > v_num;
                            }
                        } else if op == "<" {
                            if let (Ok(c_num), Ok(v_num)) = (cell.parse::<f64>(), val.parse::<f64>()) {
                                matched = c_num < v_num;
                            }
                        } else if op == "contains" {
                            matched = cell.to_lowercase().contains(&val.to_lowercase());
                        }
                        if matched {
                            filtered_rows.push(row.clone());
                        }
                    }
                }
            }
        } else {
            filtered_rows = rows.clone();
        }
        
        if filtered_rows.is_empty() {
            return;
        }
        
        // 2. Identify unique row values and column values
        let mut unique_rows = std::collections::BTreeSet::new();
        let mut unique_cols = std::collections::BTreeSet::new();
        
        for row in &filtered_rows {
            if let Some(r_idx) = self.pivot_state.config.row_dimension_idx {
                if let Some(v) = row.get(r_idx) {
                    unique_rows.insert(v.clone());
                }
            } else {
                unique_rows.insert("Grand Total".to_string());
            }
            
            if let Some(c_idx) = self.pivot_state.config.col_dimension_idx {
                if let Some(v) = row.get(c_idx) {
                    unique_cols.insert(v.clone());
                }
            } else {
                unique_cols.insert("Grand Total".to_string());
            }
        }
        
        let row_dims: Vec<String> = unique_rows.into_iter().collect();
        let col_dims: Vec<String> = unique_cols.into_iter().collect();
        
        // 3. Group values in a hashmap
        let mut groups: std::collections::HashMap<(String, String), Vec<f64>> = std::collections::HashMap::new();
        let mut groups_raw: std::collections::HashMap<(String, String), Vec<String>> = std::collections::HashMap::new();
        let mut groups_base: std::collections::HashMap<(String, String), Vec<f64>> = std::collections::HashMap::new();
        let mut groups_cond_matched: std::collections::HashMap<(String, String), Vec<bool>> = std::collections::HashMap::new();
        
        for row in &filtered_rows {
            let r_val = self.pivot_state.config.row_dimension_idx
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_else(|| "Grand Total".to_string());
                
            let c_val = self.pivot_state.config.col_dimension_idx
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_else(|| "Grand Total".to_string());
                
            let val_str = self.pivot_state.config.value_column_idx
                .and_then(|idx| row.get(idx))
                .cloned()
                .unwrap_or_else(|| "1.0".to_string());
                
            let val = val_str.parse::<f64>().unwrap_or(1.0);
            
            groups.entry((r_val.clone(), c_val.clone())).or_default().push(val);
            groups_raw.entry((r_val.clone(), c_val.clone())).or_default().push(val_str);
            
            // Denominator column for rates
            let base_val = if let Some(base_idx) = self.pivot_state.config.rate_base_column_idx {
                row.get(base_idx).and_then(|x| x.parse::<f64>().ok()).unwrap_or(1.0)
            } else {
                1.0
            };
            groups_base.entry((r_val.clone(), c_val.clone())).or_default().push(base_val);
            
            // Conditional matching for SumIf/CountIf
            let mut cond_matched = false;
            if let Some(f_idx) = self.pivot_state.config.filter_col_idx {
                let op = &self.pivot_state.config.filter_op;
                let val = &self.pivot_state.config.filter_val;
                if let Some(cell) = row.get(f_idx) {
                    if op == "=" {
                        cond_matched = cell.to_lowercase() == val.to_lowercase();
                    } else if op == "!=" {
                        cond_matched = cell.to_lowercase() != val.to_lowercase();
                    } else if op == ">" {
                        if let (Ok(c_num), Ok(v_num)) = (cell.parse::<f64>(), val.parse::<f64>()) {
                            cond_matched = c_num > v_num;
                        }
                    } else if op == "<" {
                        if let (Ok(c_num), Ok(v_num)) = (cell.parse::<f64>(), val.parse::<f64>()) {
                            cond_matched = c_num < v_num;
                        }
                    } else if op == "contains" {
                        cond_matched = cell.to_lowercase().contains(&val.to_lowercase());
                    }
                }
            } else {
                cond_matched = true;
            }
            groups_cond_matched.entry((r_val, c_val)).or_default().push(cond_matched);
        }
        
        // 4. Calculate raw aggregates
        let mut cell_values = std::collections::HashMap::new();
        
        let mut row_totals = std::collections::HashMap::new();
        let mut col_totals = std::collections::HashMap::new();
        let mut grand_total = 0.0;
        
        // For recalculating rates and ratios properly on totals:
        let mut row_numerators = std::collections::HashMap::new();
        let mut row_denominators = std::collections::HashMap::new();
        let mut col_numerators = std::collections::HashMap::new();
        let mut col_denominators = std::collections::HashMap::new();
        let mut grand_numerator = 0.0;
        let mut grand_denominator = 0.0;
        
        let is_rate_or_ratio = matches!(agg_fn, AggregationFunction::Rate | AggregationFunction::Ratio);
        
        for r_val in &row_dims {
            for c_val in &col_dims {
                let key = (r_val.clone(), c_val.clone());
                
                let num = if let Some(vals) = groups.get(&key) {
                    match agg_fn {
                        AggregationFunction::Sum | AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand => {
                            vals.iter().sum::<f64>()
                        }
                        AggregationFunction::Count => vals.len() as f64,
                        AggregationFunction::Avg => {
                            vals.iter().sum::<f64>()
                        }
                        AggregationFunction::Min => {
                            vals.iter().copied().fold(f64::INFINITY, f64::min)
                        }
                        AggregationFunction::Max => {
                            vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                        }
                        AggregationFunction::CountDistinct => {
                            if let Some(raw_vals) = groups_raw.get(&key) {
                                let unique_raw: std::collections::HashSet<&String> = raw_vals.iter().collect();
                                unique_raw.len() as f64
                            } else {
                                0.0
                            }
                        }
                        AggregationFunction::SumIf => {
                            let mut sum = 0.0;
                            if let Some(conds) = groups_cond_matched.get(&key) {
                                for (v, &cond) in vals.iter().zip(conds.iter()) {
                                    if cond {
                                        sum += v;
                                    }
                                }
                            }
                            sum
                        }
                        AggregationFunction::CountIf => {
                            let mut count = 0.0;
                            if let Some(conds) = groups_cond_matched.get(&key) {
                                for &cond in conds {
                                    if cond {
                                        count += 1.0;
                                    }
                                }
                            }
                            count
                        }
                        AggregationFunction::Rate | AggregationFunction::Ratio => {
                            vals.iter().sum::<f64>()
                        }
                    }
                } else {
                    0.0
                };
                
                let den = if let Some(base_vals) = groups_base.get(&key) {
                    base_vals.iter().sum::<f64>()
                } else {
                    0.0
                };
                
                let val = match agg_fn {
                    AggregationFunction::Rate => {
                        if den == 0.0 { 0.0 } else { (num / den) * 100.0 }
                    }
                    AggregationFunction::Ratio => {
                        if den == 0.0 { 0.0 } else { num / den }
                    }
                    AggregationFunction::Avg => {
                        let count = groups.get(&key).map(|v| v.len()).unwrap_or(0);
                        if count == 0 { 0.0 } else { num / count as f64 }
                    }
                    _ => num,
                };
                
                cell_values.insert(key.clone(), val);
                
                if !is_rate_or_ratio {
                    *row_totals.entry(r_val.clone()).or_insert(0.0) += val;
                    *col_totals.entry(c_val.clone()).or_insert(0.0) += val;
                    grand_total += val;
                } else {
                    *row_numerators.entry(r_val.clone()).or_insert(0.0) += num;
                    *row_denominators.entry(r_val.clone()).or_insert(0.0) += den;
                    *col_numerators.entry(c_val.clone()).or_insert(0.0) += num;
                    *col_denominators.entry(c_val.clone()).or_insert(0.0) += den;
                    grand_numerator += num;
                    grand_denominator += den;
                }
            }
        }
        
        // If Rate or Ratio, calculate proper mathematical totals
        if is_rate_or_ratio {
            let factor = if agg_fn == AggregationFunction::Rate { 100.0 } else { 1.0 };
            for r_val in &row_dims {
                let num = row_numerators.get(r_val).copied().unwrap_or(0.0);
                let den = row_denominators.get(r_val).copied().unwrap_or(0.0);
                let t = if den == 0.0 { 0.0 } else { (num / den) * factor };
                row_totals.insert(r_val.clone(), t);
            }
            for c_val in &col_dims {
                let num = col_numerators.get(c_val).copied().unwrap_or(0.0);
                let den = col_denominators.get(c_val).copied().unwrap_or(0.0);
                let t = if den == 0.0 { 0.0 } else { (num / den) * factor };
                col_totals.insert(c_val.clone(), t);
            }
            grand_total = if grand_denominator == 0.0 { 0.0 } else { (grand_numerator / grand_denominator) * factor };
        }
        
        // 5. Format headers
        let row_header = self.pivot_state.config.row_dimension_idx
            .and_then(|idx| cols.get(idx))
            .cloned()
            .unwrap_or_else(|| "Rows".to_string());
            
        let mut headers = vec![row_header];
        for col in &col_dims {
            headers.push(col.clone());
        }
        headers.push("Grand Total".to_string());
        self.pivot_state.pivot_headers = headers;
        
        // 6. Format table rows (including % calculations)
        for r_val in &row_dims {
            let mut row = vec![r_val.clone()];
            let mut row_sum = 0.0;
            
            for c_val in &col_dims {
                let raw_val = cell_values.get(&(r_val.clone(), c_val.clone())).copied().unwrap_or(0.0);
                
                let val = match agg_fn {
                    AggregationFunction::PercentOfRow => {
                        let r_total = row_totals.get(r_val).copied().unwrap_or(0.0);
                        if r_total == 0.0 { 0.0 } else { (raw_val / r_total) * 100.0 }
                    }
                    AggregationFunction::PercentOfCol => {
                        let c_total = col_totals.get(c_val).copied().unwrap_or(0.0);
                        if c_total == 0.0 { 0.0 } else { (raw_val / c_total) * 100.0 }
                    }
                    AggregationFunction::PercentOfGrand => {
                        if grand_total == 0.0 { 0.0 } else { (raw_val / grand_total) * 100.0 }
                    }
                    _ => raw_val,
                };
                
                row_sum += val;
                
                // Format nicely
                let formatted = match agg_fn {
                    AggregationFunction::Count | AggregationFunction::CountIf | AggregationFunction::CountDistinct => format!("{:.0}", val),
                    AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand | AggregationFunction::Rate => format!("{:.1}%", val),
                    AggregationFunction::Ratio => format!("{:.4}", val),
                    _ => format!("{:.2}", val),
                };
                row.push(formatted);
            }
            
            // Grand Total cell for row
            let formatted_total = match agg_fn {
                AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand => {
                    if agg_fn == AggregationFunction::PercentOfRow {
                        "100.0%".to_string()
                    } else {
                        format!("{:.1}%", row_sum)
                    }
                }
                AggregationFunction::Rate => {
                    let r_val_total = row_totals.get(r_val).copied().unwrap_or(0.0);
                    format!("{:.1}%", r_val_total)
                }
                AggregationFunction::Ratio => {
                    let r_val_total = row_totals.get(r_val).copied().unwrap_or(0.0);
                    format!("{:.4}", r_val_total)
                }
                AggregationFunction::Count | AggregationFunction::CountIf | AggregationFunction::CountDistinct => format!("{:.0}", row_sum),
                _ => format!("{:.2}", row_sum),
            };
            row.push(formatted_total);
            self.pivot_state.pivot_rows.push(row);
        }
        
        // 7. Add Grand Total row at bottom
        let mut total_row = vec!["Grand Total".to_string()];
        
        for c_val in &col_dims {
            let raw_val = col_totals.get(c_val).copied().unwrap_or(0.0);
            
            let val = match agg_fn {
                AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand => {
                    if agg_fn == AggregationFunction::PercentOfCol {
                        100.0
                    } else if grand_total == 0.0 {
                        0.0
                    } else {
                        (raw_val / grand_total) * 100.0
                    }
                }
                _ => raw_val,
            };
            
            let formatted = match agg_fn {
                AggregationFunction::Count | AggregationFunction::CountIf | AggregationFunction::CountDistinct => format!("{:.0}", val),
                AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand | AggregationFunction::Rate => format!("{:.1}%", val),
                AggregationFunction::Ratio => format!("{:.4}", val),
                _ => format!("{:.2}", val),
            };
            total_row.push(formatted);
        }
        
        let formatted_grand_total = match agg_fn {
            AggregationFunction::PercentOfRow | AggregationFunction::PercentOfCol | AggregationFunction::PercentOfGrand => "100.0%".to_string(),
            AggregationFunction::Rate => format!("{:.1}%", grand_total),
            AggregationFunction::Ratio => format!("{:.4}", grand_total),
            AggregationFunction::Count | AggregationFunction::CountIf | AggregationFunction::CountDistinct => format!("{:.0}", grand_total),
            _ => format!("{:.2}", grand_total),
        };
        total_row.push(formatted_grand_total);
        self.pivot_state.pivot_rows.push(total_row);
        
        // 8. Build chart data
        if self.pivot_state.pivot_rows.len() > 1 {
            let row_len = self.pivot_state.pivot_rows.len();
            let val_idx = self.pivot_state.pivot_headers.len() - 1; // Grand Total column
            
            for i in 0..(row_len - 1) { // exclude last total row
                let prow = &self.pivot_state.pivot_rows[i];
                let label = prow[0].clone();
                let val_str = prow.get(val_idx).map(|x| x.as_str()).unwrap_or("0");
                let clean_val = val_str.replace('%', "");
                let value = clean_val.parse::<f64>().unwrap_or(0.0).round() as u64;
                self.pivot_state.pivot_chart_data.push(BiBarData { label, value });
            }
        }
    }

    pub fn move_bi_selector(&mut self, up: bool) {
        if up {
            if self.pivot_state.active_selector_idx > 0 {
                self.pivot_state.active_selector_idx -= 1;
            } else {
                self.pivot_state.active_selector_idx = 10;
            }
        } else {
            if self.pivot_state.active_selector_idx < 10 {
                self.pivot_state.active_selector_idx += 1;
            } else {
                self.pivot_state.active_selector_idx = 0;
            }
        }
    }

    pub fn cycle_bi_selector(&mut self, next: bool) {
        let headers = if self.pivot_state.config.bi_source_related {
            &self.related_headers
        } else {
            &self.result_headers
        };
        let num_cols = headers.len();
        let idx = self.pivot_state.active_selector_idx;
        match idx {
            0 => { // Rows
                self.pivot_state.config.row_dimension_idx = cycle_option_idx(self.pivot_state.config.row_dimension_idx, num_cols, next);
            }
            1 => { // Columns
                self.pivot_state.config.col_dimension_idx = cycle_option_idx(self.pivot_state.config.col_dimension_idx, num_cols, next);
            }
            2 => { // Values
                self.pivot_state.config.value_column_idx = cycle_option_idx(self.pivot_state.config.value_column_idx, num_cols, next);
            }
            3 => { // Aggr. Function
                self.pivot_state.config.agg_fn = cycle_agg_fn(self.pivot_state.config.agg_fn, next);
            }
            4 => { // Filter Column
                self.pivot_state.config.filter_col_idx = cycle_option_idx(self.pivot_state.config.filter_col_idx, num_cols, next);
            }
            5 => { // Filter Operator
                let ops = vec!["=", "!=", ">", "<", "contains"];
                if let Some(pos) = ops.iter().position(|&x| x == self.pivot_state.config.filter_op) {
                    let next_pos = if next {
                        (pos + 1) % ops.len()
                    } else {
                        (pos + ops.len() - 1) % ops.len()
                    };
                    self.pivot_state.config.filter_op = ops[next_pos].to_string();
                } else {
                    self.pivot_state.config.filter_op = "=".to_string();
                }
            }
            6 => { // Filter Value
                // Text input is managed directly by keystrokes
            }
            7 => { // Chart Representation
                self.pivot_state.config.chart_type = cycle_chart_type(self.pivot_state.config.chart_type, next);
            }
            8 => { // Recalculate Mode
                self.pivot_state.config.auto_recalc = !self.pivot_state.config.auto_recalc;
            }
            9 => { // Rate Base Column (Denom.)
                self.pivot_state.config.rate_base_column_idx = cycle_option_idx(self.pivot_state.config.rate_base_column_idx, num_cols, next);
            }
            10 => { // BI Data Source
                self.pivot_state.config.bi_source_related = !self.pivot_state.config.bi_source_related;
                self.pivot_state.config.row_dimension_idx = None;
                self.pivot_state.config.col_dimension_idx = None;
                self.pivot_state.config.value_column_idx = None;
                self.pivot_state.config.filter_col_idx = None;
                self.pivot_state.config.rate_base_column_idx = None;
            }
            _ => {}
        }
        
        if self.pivot_state.config.auto_recalc {
            self.recalculate_pivot();
        }
    }

    // Modal Row Editor Builders
    pub fn open_edit_row_modal(&mut self) {
        if let Some(row_idx) = self.selected_row_idx {
            if row_idx < self.result_rows.len() {
                self.modal_fields.clear();
                let row_cells = &self.result_rows[row_idx];
                for (i, header) in self.result_headers.iter().enumerate() {
                    let val = row_cells.get(i).cloned().unwrap_or_default();
                    self.modal_fields.push((header.clone(), val));
                }
                
                // Track PK value if available
                if let Some(ref pk_col) = self.primary_key {
                    if let Some(pos) = self.result_headers.iter().position(|x| x.to_lowercase() == pk_col.to_lowercase()) {
                        self.selected_row_pk_val = row_cells.get(pos).cloned();
                    }
                }
                self.show_edit_modal = true;
                self.active_modal_field_idx = 0;
                self.active_pane = ActivePane::ModalEditor;
            }
        }
    }

    pub fn open_add_row_modal(&mut self) {
        self.modal_fields.clear();
        for header in &self.result_headers {
            self.modal_fields.push((header.clone(), "".to_string()));
        }
        self.show_add_modal = true;
        self.active_modal_field_idx = 0;
        self.active_pane = ActivePane::ModalEditor;
    }

    pub fn compile_update_statement(&self) -> Option<String> {
        let table = self.active_table_name.as_ref()?;
        let pk_col = self.primary_key.as_ref()?;
        let pk_val = self.selected_row_pk_val.as_ref()?;

        let mut sets = Vec::new();
        for (col, val) in &self.modal_fields {
            if col == pk_col {
                continue; // Do not update primary key
            }
            let escaped = val.replace('\'', "''");
            sets.push(format!("{} = '{}'", col, escaped));
        }

        if sets.is_empty() {
            return None;
        }

        Some(format!(
            "UPDATE {} SET {} WHERE {} = '{}';",
            table,
            sets.join(", "),
            pk_col,
            pk_val.replace('\'', "''")
        ))
    }

    pub fn compile_insert_statement(&self) -> Option<String> {
        let table = self.active_table_name.as_ref()?;
        let mut cols = Vec::new();
        let mut vals = Vec::new();

        for (col, val) in &self.modal_fields {
            cols.push(col.clone());
            let escaped = val.replace('\'', "''");
            vals.push(format!("'{}'", escaped));
        }

        Some(format!(
            "INSERT INTO {} ({}) VALUES ({});",
            table,
            cols.join(", "),
            vals.join(", ")
        ))
    }

    pub fn compile_delete_statement(&self) -> Option<String> {
        let table = self.active_table_name.as_ref()?;
        let pk_col = self.primary_key.as_ref()?;
        let pk_val = self.selected_row_pk_val.as_ref()?;

        Some(format!(
            "DELETE FROM {} WHERE {} = '{}';",
            table,
            pk_col,
            pk_val.replace('\'', "''")
        ))
    }

    // ---- Searcher (client-side row filter over the currently loaded grid) ----

    /// Returns true if the given row matches the active search query (case-insensitive
    /// substring on any cell). An empty query matches everything.
    pub fn row_matches_search(&self, row: &[String]) -> bool {
        if self.search_query.is_empty() {
            return true;
        }
        let needle = self.search_query.to_lowercase();
        row.iter().any(|cell| cell.to_lowercase().contains(&needle))
    }

    /// Indices into `result_rows` that are currently visible given the search filter.
    pub fn visible_row_indices(&self) -> Vec<usize> {
        self.result_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| self.row_matches_search(row))
            .map(|(i, _)| i)
            .collect()
    }

    /// Move the grid selection to the previous/next *visible* row. Returns true if
    /// the selection actually moved (so callers can refresh related data).
    pub fn step_visible_selection(&mut self, forward: bool) -> bool {
        let visible = self.visible_row_indices();
        if visible.is_empty() {
            self.selected_row_idx = None;
            return false;
        }
        let current = self.selected_row_idx.unwrap_or(visible[0]);
        // Position of current selection within the visible list (fallback to edge).
        let pos = visible.iter().position(|&i| i == current);
        let new_idx = match pos {
            Some(p) if forward => {
                if p + 1 < visible.len() { visible[p + 1] } else { return false; }
            }
            Some(p) => {
                if p > 0 { visible[p - 1] } else { return false; }
            }
            None => visible[0],
        };
        if Some(new_idx) != self.selected_row_idx {
            self.selected_row_idx = Some(new_idx);
            true
        } else {
            false
        }
    }

    /// Ensure the current selection points at a visible row (used after editing the filter).
    pub fn clamp_selection_to_visible(&mut self) {
        let visible = self.visible_row_indices();
        if visible.is_empty() {
            self.selected_row_idx = None;
            return;
        }
        let still_visible = self
            .selected_row_idx
            .map(|i| visible.contains(&i))
            .unwrap_or(false);
        if !still_visible {
            self.selected_row_idx = Some(visible[0]);
        }
    }

    // ---- Describe / schema overlay ----

    /// Best-effort column type inference from the sampled cell values.
    fn infer_column_type(samples: &[&String]) -> &'static str {
        let mut seen = false;
        let mut all_int = true;
        let mut all_num = true;
        for s in samples {
            let t = s.trim();
            if t.is_empty() || t.eq_ignore_ascii_case("null") {
                continue;
            }
            seen = true;
            if t.parse::<i64>().is_err() {
                all_int = false;
            }
            if t.parse::<f64>().is_err() {
                all_num = false;
            }
        }
        if !seen {
            "empty/null"
        } else if all_int {
            "integer"
        } else if all_num {
            "decimal"
        } else {
            "text"
        }
    }

    /// Rebuild the describe/schema table from the current headers, primary key,
    /// relationships and a sample of the loaded rows. Does not hit the database.
    pub fn rebuild_describe(&mut self) {
        self.describe_headers = vec![
            "#".to_string(),
            "Column".to_string(),
            "Type".to_string(),
            "Key".to_string(),
            "Sample".to_string(),
        ];
        self.describe_rows.clear();

        let pk = self.primary_key.clone().unwrap_or_default();
        let sample_limit = self.result_rows.len().min(25);

        for (idx, header) in self.result_headers.iter().enumerate() {
            let samples: Vec<&String> = self
                .result_rows
                .iter()
                .take(sample_limit)
                .filter_map(|r| r.get(idx))
                .collect();
            let ty = Self::infer_column_type(&samples);

            let mut key_role = String::new();
            if !pk.is_empty() && header.eq_ignore_ascii_case(&pk) {
                key_role = "PK".to_string();
            }
            for rel in &self.relationships {
                if rel.is_parent && rel.active_col.eq_ignore_ascii_case(header) {
                    let fk = format!("FK→{}.{}", rel.target_table, rel.target_col);
                    key_role = if key_role.is_empty() { fk } else { format!("{},{}", key_role, fk) };
                }
            }

            let sample = samples
                .iter()
                .find(|s| {
                    let t = s.trim();
                    !t.is_empty() && !t.eq_ignore_ascii_case("null")
                })
                .map(|s| {
                    let mut v = (*s).clone();
                    if v.chars().count() > 24 {
                        v = format!("{}…", v.chars().take(23).collect::<String>());
                    }
                    v
                })
                .unwrap_or_default();

            self.describe_rows.push(vec![
                (idx + 1).to_string(),
                header.clone(),
                ty.to_string(),
                key_role,
                sample,
            ]);
        }
    }

    pub fn toggle_describe(&mut self) {
        if !self.show_describe {
            self.rebuild_describe();
        }
        self.show_describe = !self.show_describe;
    }

    /// Indices of the sidebar entries (tables or databases, depending on the
    /// current mode) that match the quick-find filter.
    pub fn sidebar_visible_indices(&self) -> Vec<usize> {
        let src = if self.show_db_list { &self.databases } else { &self.tables };
        let f = self.sidebar_filter.to_lowercase();
        src.iter()
            .enumerate()
            .filter(|(_, name)| f.is_empty() || name.to_lowercase().contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    /// Move the sidebar selection within the filtered entries.
    pub fn sidebar_step(&mut self, down: bool) {
        let vis = self.sidebar_visible_indices();
        if vis.is_empty() {
            return;
        }
        let sel = if self.show_db_list { self.selected_db_idx } else { self.selected_table_idx };
        let pos = sel.and_then(|s| vis.iter().position(|&i| i == s));
        let new_pos = match pos {
            Some(p) if down => (p + 1).min(vis.len() - 1),
            Some(p) => p.saturating_sub(1),
            None => 0,
        };
        let new_sel = Some(vis[new_pos]);
        if self.show_db_list {
            self.selected_db_idx = new_sel;
        } else {
            self.selected_table_idx = new_sel;
        }
    }

    /// Keep the sidebar selection on a visible entry while the filter changes.
    pub fn sidebar_clamp_selection(&mut self) {
        let vis = self.sidebar_visible_indices();
        let sel = if self.show_db_list { self.selected_db_idx } else { self.selected_table_idx };
        let ok = sel.map(|s| vis.contains(&s)).unwrap_or(false);
        if !ok {
            let new_sel = vis.first().copied();
            if self.show_db_list {
                self.selected_db_idx = new_sel;
            } else {
                self.selected_table_idx = new_sel;
            }
        }
    }

    /// Open the row-trace overlay for the selected result row and build the
    /// worker request that walks its full FK lineage (ancestors + descendants).
    pub fn open_row_trace(&mut self) -> Option<DbRequest> {
        if self.show_trace {
            self.close_row_trace();
            return None;
        }
        let table = self.active_table_name.clone()?;
        let idx = self.selected_row_idx?;
        let row = self.result_rows.get(idx)?.clone();
        self.show_describe = false;
        self.show_trace = true;
        self.trace_loading = true;
        self.trace_root = None;
        self.trace_error = None;
        self.trace_json_mode = false;
        self.trace_scroll = 0;
        Some(DbRequest::TraceRow {
            table,
            columns: self.result_headers.clone(),
            values: row,
        })
    }

    pub fn close_row_trace(&mut self) {
        self.show_trace = false;
        self.trace_loading = false;
        self.trace_root = None;
        self.trace_error = None;
        self.trace_scroll = 0;
    }

    /// Run a toolbar action (shared by mouse clicks and keyboard shortcuts).
    /// Returns a DbRequest when the action needs the worker (e.g. refresh/back).
    pub fn trigger_toolbar_action(&mut self, action: ToolbarAction) -> Option<DbRequest> {
        match action {
            ToolbarAction::Search => {
                self.active_pane = ActivePane::QueryResults;
                self.show_describe = false;
                let is_dbf = self.active_engine == ActiveEngine::LocalJson && self.conn_fields.json_path.ends_with(".dbf");
                let is_document_view = (self.active_engine == ActiveEngine::MongoDb || self.active_engine == ActiveEngine::LocalJson) && !is_dbf;
                if !is_document_view {
                    self.search_active = true;
                }
                None
            }
            ToolbarAction::Describe => {
                self.active_pane = ActivePane::QueryResults;
                self.toggle_describe();
                None
            }
            ToolbarAction::Trace => {
                self.active_pane = ActivePane::QueryResults;
                self.open_row_trace()
            }
            ToolbarAction::Edit => {
                self.active_pane = ActivePane::QueryResults;
                self.open_edit_row_modal();
                None
            }
            ToolbarAction::Add => {
                self.active_pane = ActivePane::QueryResults;
                self.open_add_row_modal();
                None
            }
            ToolbarAction::Delete => {
                self.active_pane = ActivePane::QueryResults;
                if let Some(idx) = self.selected_row_idx {
                    if idx < self.result_rows.len() {
                        self.show_describe = false;
                        self.show_delete_confirm = true;
                        if let Some(ref pk_col) = self.primary_key {
                            if let Some(pos) = self.result_headers.iter().position(|x| x.to_lowercase() == pk_col.to_lowercase()) {
                                self.selected_row_pk_val = self.result_rows[idx].get(pos).cloned();
                            }
                        }
                    }
                }
                None
            }
            ToolbarAction::Refresh => {
                let q = if !self.last_executed_query.trim().is_empty() {
                    self.last_executed_query.clone()
                } else {
                    self.sql_console_input.clone()
                };
                if q.trim().is_empty() {
                    None
                } else {
                    self.connecting = true;
                    Some(DbRequest::ExecuteQuery(q))
                }
            }
            ToolbarAction::ToggleChart => {
                if self.bi_chartable {
                    self.bi_mode_enabled = !self.bi_mode_enabled;
                    self.active_pane = ActivePane::QueryResults;
                }
                None
            }
            ToolbarAction::ToggleRelated => {
                self.show_related_split = !self.show_related_split;
                None
            }
            ToolbarAction::Databases => {
                self.active_pane = ActivePane::Sidebar;
                self.show_db_list = true;
                self.connecting = true;
                self.conn_status_msg = "Loading databases...".to_string();
                Some(DbRequest::LoadDatabases)
            }
            ToolbarAction::Back => {
                if let Some(prev) = self.exploration_history.pop() {
                    self.active_table_name = prev.table_name.clone();
                    self.sql_console_input = prev.query.clone();
                    self.sql_cursor_pos = prev.query.len();
                    self.active_relationship_idx = prev.active_relationship_idx;
                    self.show_related_split = prev.show_related_split;
                    self.active_pane = ActivePane::QueryResults;
                    self.connecting = true;
                    Some(DbRequest::ExecuteQuery(prev.query))
                } else {
                    None
                }
            }
            ToolbarAction::Disconnect => {
                self.connected = false;
                self.active_pane = ActivePane::EngineSelector;
                self.conn_status_msg = "Ready to connect".to_string();
                None
            }
        }
    }

    pub fn handle_mouse_click(&mut self, col: u16, row: u16) -> Option<DbRequest> {
        let is_inside = |rect: Rect| -> bool {
            col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
        };

        // 0. Row-detail overlay swallows clicks; clicking outside closes it.
        if self.show_row_detail {
            if let Some(rect) = self.rect_row_detail {
                if !is_inside(rect) {
                    self.show_row_detail = false;
                }
            } else {
                self.show_row_detail = false;
            }
            return None;
        }

        // 0. Trace overlay swallows clicks; clicking outside closes it.
        if self.show_trace {
            if let Some(rect) = self.rect_trace {
                if !is_inside(rect) {
                    self.close_row_trace();
                }
            } else {
                self.close_row_trace();
            }
            return None;
        }

        // 0. Describe overlay swallows clicks; clicking outside closes it.
        if self.show_describe {
            if let Some(rect) = self.rect_describe {
                if !is_inside(rect) {
                    self.show_describe = false;
                }
            } else {
                self.show_describe = false;
            }
            return None;
        }

        // 0a. Action toolbar buttons.
        let hit = self
            .toolbar_buttons
            .iter()
            .find(|(rect, _)| is_inside(*rect))
            .map(|(_, action)| *action);
        if let Some(action) = hit {
            return self.trigger_toolbar_action(action);
        }

        // 0b. Clicking the always-on query bar jumps to the SQL console.
        if let Some(rect) = self.rect_query_bar {
            if is_inside(rect) {
                self.active_pane = ActivePane::SqlConsole;
                return None;
            }
        }

        // 1. Header Tabs
        if let Some(rect) = self.rect_header_tabs {
            if is_inside(rect) {
                // Determine which tab was clicked: "[1] Tables/Schemas List", "[2] SQL Console", "[3] Visual BI Dash"
                let tab_width = rect.width / 3;
                if tab_width > 0 {
                    let tab_idx = (col - rect.x) / tab_width;
                    match tab_idx {
                        0 => {
                            self.active_pane = ActivePane::Sidebar;
                            self.bi_mode_enabled = false;
                        }
                        1 => {
                            self.active_pane = ActivePane::SqlConsole;
                        }
                        2 => {
                            if self.bi_chartable {
                                self.bi_mode_enabled = true;
                                self.active_pane = ActivePane::QueryResults;
                            }
                        }
                        _ => {}
                    }
                }
                return None;
            }
        }

        // 2. Sidebar
        if let Some(rect) = self.rect_sidebar {
            if is_inside(rect) {
                self.active_pane = ActivePane::Sidebar;
                // rect represents the inner area of the sidebar block
                let click_idx = row as i32 - rect.y as i32;
                if click_idx >= 0 {
                    // Map the clicked line through the quick-find filter.
                    let vis = self.sidebar_visible_indices();
                    let click_idx = match vis.get(click_idx as usize) {
                        Some(&real) => real,
                        None => return None,
                    };
                    if self.show_db_list {
                        if click_idx < self.databases.len() {
                            self.selected_db_idx = Some(click_idx);
                            let db_name = self.databases[click_idx].clone();
                            self.connecting = true;
                            self.conn_status_msg = format!("Switching database to {}...", db_name);
                            self.show_db_list = false;
                            return Some(DbRequest::SelectDatabase(db_name));
                        }
                    } else {
                        if click_idx < self.tables.len() {
                            self.selected_table_idx = Some(click_idx);
                            let table = &self.tables[click_idx];
                            self.active_table_name = Some(table.clone());
                            self.exploration_history.clear();
                            let query = match self.active_engine {
                                ActiveEngine::MariaDb | ActiveEngine::PostgreSql | ActiveEngine::Sqlite => {
                                    format!("SELECT * FROM {} LIMIT 50;", table)
                                }
                                ActiveEngine::MongoDb => {
                                    format!("{}|{{}}", table)
                                }
                                ActiveEngine::Neo4j => {
                                    format!("MATCH (n:{}) RETURN n LIMIT 25;", table)
                                }
                                ActiveEngine::LocalJson => "".to_string(),
                            };
                            self.sql_console_input = query.clone();
                            self.sql_cursor_pos = query.len();
                            self.connecting = true;
                            return Some(DbRequest::ExecuteQuery(query));
                        }
                    }
                }
                return None;
            }
        }

        // 3. SQL Console
        if let Some(rect) = self.rect_sql_console {
            if is_inside(rect) {
                self.active_pane = ActivePane::SqlConsole;
                return None;
            }
        }

        // 4. BI Configurator / Results
        if self.bi_mode_enabled {
            if let Some(rect) = self.rect_bi_config {
                if is_inside(rect) {
                    self.active_pane = ActivePane::QueryResults;
                    // There are 11 config selectors: 0 to 10
                    // Each setting + border takes 2 lines
                    let click_idx = (row as i32 - rect.y as i32) / 2;
                    if click_idx >= 0 && click_idx <= 10 {
                        self.pivot_state.active_selector_idx = click_idx as usize;
                    }
                    return None;
                }
            }
        } else {
            // 5. Data View (Main Table/Tree)
            if let Some(rect) = self.rect_data_view {
                if is_inside(rect) {
                    self.active_pane = ActivePane::QueryResults;
                    let is_dbf = self.active_engine == ActiveEngine::LocalJson && self.conn_fields.json_path.ends_with(".dbf");
                    let is_document_view = (self.active_engine == ActiveEngine::MongoDb || self.active_engine == ActiveEngine::LocalJson) && !is_dbf;
                    // Account for the pane's top border + header row + header bottom-margin.
                    let header_offset = if is_document_view { 1 } else { 3 };
                    let click_idx = row as i32 - rect.y as i32 - header_offset;
                    if click_idx >= 0 {
                        let click_idx = click_idx as usize;
                        if is_document_view {
                            if click_idx < self.flat_tree_rows.len() {
                                self.selected_tree_row_idx = Some(click_idx);
                            }
                        } else {
                            // Map the visible (filtered) position back to the real row index.
                            let visible = self.visible_row_indices();
                            if click_idx < visible.len() {
                                self.selected_row_idx = Some(visible[click_idx]);
                            }
                        }
                    }
                    return None;
                }
            }

            // 6. Related Data List
            if let Some(rect) = self.rect_related_list {
                if is_inside(rect) {
                    self.active_pane = ActivePane::RelatedDataList;
                    let click_idx = row as i32 - (rect.y + 1) as i32;
                    if click_idx >= 0 {
                        let click_idx = click_idx as usize;
                        if click_idx < self.relationships.len() {
                            self.active_relationship_idx = click_idx;
                            self.related_selected_row_idx = None;
                            if self.show_related_split {
                                if let Some(rel) = self.relationships.get(self.active_relationship_idx) {
                                    if let Some(row_idx) = self.selected_row_idx {
                                        if let Some(row_cells) = self.result_rows.get(row_idx) {
                                            if let Some(col_pos) = self.result_headers.iter().position(|c| c.to_lowercase() == rel.active_col.to_lowercase()) {
                                                if let Some(active_val) = row_cells.get(col_pos) {
                                                    return Some(DbRequest::LoadRelatedData {
                                                        relationship: rel.clone(),
                                                        active_row_val: active_val.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return None;
                }
            }

            // 7. Related Data Grid
            if let Some(rect) = self.rect_related_grid {
                if is_inside(rect) {
                    self.active_pane = ActivePane::RelatedDataGrid;
                    let click_idx = row as i32 - (rect.y + 2) as i32;
                    if click_idx >= 0 {
                        let click_idx = click_idx as usize;
                        if click_idx < self.related_rows.len() {
                            self.related_selected_row_idx = Some(click_idx);
                        }
                    }
                    return None;
                }
            }
        }

        None
    }
}

pub fn json_to_tree_item(key: &str, value: &Value, depth: usize, mut path: Vec<String>) -> TreeItem {
    path.push(key.to_string());
    match value {
        Value::Null => TreeItem {
            key: key.to_string(),
            value_summary: "null".to_string(),
            depth,
            is_expanded: false,
            children: vec![],
            path,
        },
        Value::Bool(b) => TreeItem {
            key: key.to_string(),
            value_summary: b.to_string(),
            depth,
            is_expanded: false,
            children: vec![],
            path,
        },
        Value::Number(n) => TreeItem {
            key: key.to_string(),
            value_summary: n.to_string(),
            depth,
            is_expanded: false,
            children: vec![],
            path,
        },
        Value::String(s) => TreeItem {
            key: key.to_string(),
            value_summary: format!("\"{}\"", s),
            depth,
            is_expanded: false,
            children: vec![],
            path,
        },
        Value::Array(arr) => {
            let mut children = vec![];
            for (i, val) in arr.iter().enumerate() {
                children.push(json_to_tree_item(&i.to_string(), val, depth + 1, path.clone()));
            }
            TreeItem {
                key: key.to_string(),
                value_summary: format!("[Array ({} items)]", arr.len()),
                depth,
                is_expanded: false,
                children,
                path,
            }
        }
        Value::Object(obj) => {
            let mut children = vec![];
            for (k, val) in obj.iter() {
                children.push(json_to_tree_item(k, val, depth + 1, path.clone()));
            }
            TreeItem {
                key: key.to_string(),
                value_summary: format!("{{Object ({} keys)}}", obj.len()),
                depth,
                is_expanded: false,
                children,
                path,
            }
        }
    }
}

pub fn toggle_tree_node(items: &mut [TreeItem], path: &[String]) -> bool {
    for item in items.iter_mut() {
        if item.path == path {
            item.is_expanded = !item.is_expanded;
            return true;
        }
        if toggle_tree_node(&mut item.children, path) {
            return true;
        }
    }
    false
}

fn cycle_option_idx(current: Option<usize>, num_cols: usize, next: bool) -> Option<usize> {
    if num_cols == 0 {
        return None;
    }
    match current {
        None => {
            if next {
                Some(0)
            } else {
                Some(num_cols - 1)
            }
        }
        Some(idx) => {
            if next {
                if idx + 1 < num_cols {
                    Some(idx + 1)
                } else {
                    None
                }
            } else {
                if idx > 0 {
                    Some(idx - 1)
                } else {
                    None
                }
            }
        }
    }
}

fn cycle_agg_fn(current: AggregationFunction, next: bool) -> AggregationFunction {
    use AggregationFunction::*;
    let fns = [
        Sum,
        Count,
        Avg,
        Min,
        Max,
        CountDistinct,
        PercentOfRow,
        PercentOfCol,
        PercentOfGrand,
        SumIf,
        CountIf,
        Rate,
        Ratio,
    ];
    let pos = fns.iter().position(|&x| x == current).unwrap_or(0);
    let next_pos = if next {
        (pos + 1) % fns.len()
    } else {
        (pos + fns.len() - 1) % fns.len()
    };
    fns[next_pos]
}

fn cycle_chart_type(current: BiChartType, next: bool) -> BiChartType {
    use BiChartType::*;
    let types = [TableOnly, Bar, Sparkline];
    let pos = types.iter().position(|&x| x == current).unwrap_or(0);
    let next_pos = if next {
        (pos + 1) % types.len()
    } else {
        (pos + types.len() - 1) % types.len()
    };
    types[next_pos]
}
