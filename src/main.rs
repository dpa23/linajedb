mod app;
mod db;
mod ui;

use app::{ActiveEngine, ActivePane, AppState, ConnectionMode, FormField, ExplorationState};
use db::{DbRequest, DbResponse, DbWorker};
use ui::draw_ui;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, panic, time::Duration};
use tokio::sync::mpsc;

fn load_related_data_if_needed(state: &AppState) -> Option<DbRequest> {
    if !state.show_related_split || state.relationships.is_empty() {
        return None;
    }
    let rel = state.relationships.get(state.active_relationship_idx)?;
    let row_idx = state.selected_row_idx?;
    let row = state.result_rows.get(row_idx)?;
    let col_pos = state.result_headers.iter().position(|c| c.to_lowercase() == rel.active_col.to_lowercase())?;
    let active_val = row.get(col_pos)?;
    Some(DbRequest::LoadRelatedData {
        relationship: rel.clone(),
        active_row_val: active_val.clone(),
    })
}

fn extract_active_table(query: &str, engine: ActiveEngine) -> Option<String> {
    match engine {
        ActiveEngine::MariaDb | ActiveEngine::PostgreSql | ActiveEngine::Sqlite => {
            let sql_clean = query.replace('\n', " ").replace('\r', " ");
            let tokens: Vec<String> = sql_clean
                .split_whitespace()
                .map(|t| t.trim_matches(|c| c == ';' || c == '`' || c == '"' || c == '[' || c == ']' || c == '\'').to_string())
                .collect();

            let from_idx = tokens.iter().position(|t| t.to_lowercase() == "from")?;
            if from_idx + 1 < tokens.len() {
                let table_raw = &tokens[from_idx + 1];
                let table_parts: Vec<&str> = table_raw.split('.').collect();
                let table_name = table_parts.last()?.to_string();
                if !table_name.is_empty() {
                    return Some(table_name);
                }
            }
            None
        }
        ActiveEngine::MongoDb => {
            let parts: Vec<&str> = query.split('|').collect();
            if !parts.is_empty() && !parts[0].trim().is_empty() {
                Some(parts[0].trim().to_string())
            } else {
                None
            }
        }
        ActiveEngine::Neo4j => {
            if let Some(colon_idx) = query.find(':') {
                let after_colon = &query[colon_idx + 1..];
                let label: String = after_colon
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !label.is_empty() {
                    Some(label)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal and alternate screen
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Establish panic hook to ensure terminal state is restored on crashes
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, event::DisableMouseCapture);
        original_hook(panic_info);
    }));

    // Setup communication channels with database worker thread
    let (db_tx, mut db_rx) = mpsc::channel(100);
    let (app_tx, app_rx) = mpsc::channel(100);

    // Spawn async database manager
    DbWorker::spawn(app_rx, db_tx);

    let mut state = AppState::new();

    // Event Loop
    while !state.should_quit {
        // Draw Interface
        terminal.draw(|f| draw_ui(f, &mut state))?;

        // Handle database responses asynchronously
        while let Ok(res) = db_rx.try_recv() {
            match &res {
                DbResponse::Tables(tbls) => {
                    state.set_response(res.clone());
                    if !tbls.is_empty() {
                        let table = tbls[0].clone();
                        let query = match state.active_engine {
                            ActiveEngine::MariaDb | ActiveEngine::PostgreSql | ActiveEngine::Sqlite => {
                                Some(format!("SELECT * FROM {} LIMIT 50;", table))
                            }
                            ActiveEngine::MongoDb => {
                                Some(format!("{}|{{}}", table))
                            }
                            ActiveEngine::Neo4j => {
                                Some(format!("MATCH (n:{}) RETURN n LIMIT 25;", table))
                            }
                            ActiveEngine::LocalJson => None,
                        };
                        if let Some(q) = query {
                            state.connecting = true;
                            let _ = app_tx.send(DbRequest::ExecuteQuery(q)).await;
                            let _ = app_tx.send(DbRequest::LoadMetadata { table }).await;
                        }
                    }
                }
                DbResponse::Metadata { .. } => {
                    state.set_response(res.clone());
                    if let Some(req) = load_related_data_if_needed(&state) {
                        let _ = app_tx.send(req).await;
                    }
                }
                DbResponse::Connected => {
                    state.set_response(res.clone());
                    state.connecting = true;
                    state.show_db_list = true;
                    state.active_pane = ActivePane::Sidebar;
                    let _ = app_tx.send(DbRequest::LoadDatabases).await;
                }
                DbResponse::DatabaseSelected => {
                    state.set_response(res.clone());
                    state.connecting = true;
                    state.conn_status_msg = "Loading tables...".to_string();
                    let _ = app_tx.send(DbRequest::LoadTables).await;
                }
                DbResponse::Databases(_) => {
                    state.set_response(res.clone());
                }
                _ => {
                    if let Some(ref select_query) = state.mutation_in_progress {
                        match &res {
                            DbResponse::Error(err) => {
                                state.conn_status_msg = format!("Error executing change: {}", err);
                                state.mutation_in_progress = None;
                                state.connecting = false;
                            }
                            _ => {
                                let query = select_query.clone();
                                state.mutation_in_progress = None;
                                let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                            }
                        }
                    } else {
                        state.set_response(res);
                    }
                }
            }
        }

        // Poll keyboard and mouse inputs
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                // Global hotkeys
                match (key.code, key.modifiers) {
                    (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                        state.should_quit = true;
                        break;
                    }
                    (KeyCode::Tab, KeyModifiers::NONE) => {
                        state.cycle_focus();
                        continue;
                    }
                    (KeyCode::BackTab, KeyModifiers::NONE) => {
                        state.cycle_focus_back();
                        continue;
                    }
                    _ => {}
                }

                if state.connected && state.active_pane != ActivePane::SqlConsole && state.active_pane != ActivePane::ModalEditor && !state.show_delete_confirm && !state.search_active && !state.show_describe {
                    match key.code {
                        KeyCode::Char('1') => {
                            state.active_pane = ActivePane::Sidebar;
                            state.bi_mode_enabled = false;
                            continue;
                        }
                        KeyCode::Char('2') => {
                            state.active_pane = ActivePane::SqlConsole;
                            continue;
                        }
                        KeyCode::Char('3') => {
                            if state.bi_chartable {
                                state.bi_mode_enabled = true;
                                state.active_pane = ActivePane::QueryResults;
                            }
                            continue;
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if state.active_pane == ActivePane::EngineSelector && state.connection_mode == ConnectionMode::Profiles {
                                state.should_quit = true;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                // Pane-specific navigation
                match state.active_pane {
                    ActivePane::EngineSelector => {
                        match key.code {
                            KeyCode::F(2) => state.connection_mode = ConnectionMode::Profiles,
                            KeyCode::F(3) => state.connection_mode = ConnectionMode::Form,
                            KeyCode::F(4) => state.connection_mode = ConnectionMode::RawUrl,
                            KeyCode::Left => state.select_prev_engine(),
                            KeyCode::Right => state.select_next_engine(),
                            KeyCode::Up => {
                                match state.connection_mode {
                                    ConnectionMode::Profiles => {
                                        let matching: Vec<usize> = state.profiles
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, p)| p.engine == state.active_engine)
                                            .map(|(i, _)| i)
                                            .collect();
                                        if !matching.is_empty() {
                                            let pos = matching.iter().position(|&x| x == state.selected_profile_idx).unwrap_or(0);
                                            if pos > 0 {
                                                state.selected_profile_idx = matching[pos - 1];
                                            }
                                        }
                                    }
                                    ConnectionMode::Form => {
                                        state.active_form_field = match state.active_form_field {
                                            FormField::Host => FormField::Db,
                                            FormField::Port => FormField::Host,
                                            FormField::User => FormField::Port,
                                            FormField::Pass => FormField::User,
                                            FormField::Db => FormField::Pass,
                                        };
                                    }
                                    ConnectionMode::RawUrl => {}
                                }
                            }
                            KeyCode::Down => {
                                match state.connection_mode {
                                    ConnectionMode::Profiles => {
                                        let matching: Vec<usize> = state.profiles
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, p)| p.engine == state.active_engine)
                                            .map(|(i, _)| i)
                                            .collect();
                                        if !matching.is_empty() {
                                            let pos = matching.iter().position(|&x| x == state.selected_profile_idx).unwrap_or(0);
                                            if pos < matching.len() - 1 {
                                                state.selected_profile_idx = matching[pos + 1];
                                            }
                                        }
                                    }
                                    ConnectionMode::Form => {
                                        state.active_form_field = match state.active_form_field {
                                            FormField::Host => FormField::Port,
                                            FormField::Port => FormField::User,
                                            FormField::User => FormField::Pass,
                                            FormField::Pass => FormField::Db,
                                            FormField::Db => FormField::Host,
                                        };
                                    }
                                    ConnectionMode::RawUrl => {}
                                }
                            }
                            KeyCode::Enter => {
                                if !state.connecting {
                                    state.connecting = true;
                                    state.conn_status_msg = "Connecting...".to_string();
                                    let req = DbRequest::Connect(state.active_connection_config());
                                    let _ = app_tx.send(req).await;
                                }
                            }
                            KeyCode::Char(c) => {
                                match state.connection_mode {
                                    ConnectionMode::Profiles => {}
                                    ConnectionMode::Form => {
                                        match state.active_form_field {
                                            FormField::Host => state.form_fields.host.push(c),
                                            FormField::Port => state.form_fields.port.push(c),
                                            FormField::User => state.form_fields.user.push(c),
                                            FormField::Pass => state.form_fields.pass.push(c),
                                            FormField::Db => state.form_fields.db_or_path.push(c),
                                        }
                                    }
                                    ConnectionMode::RawUrl => {
                                        match state.active_engine {
                                            ActiveEngine::MariaDb => state.conn_fields.mysql_url.push(c),
                                            ActiveEngine::PostgreSql => state.conn_fields.postgres_url.push(c),
                                            ActiveEngine::Sqlite => state.conn_fields.sqlite_path.push(c),
                                            ActiveEngine::MongoDb => state.conn_fields.mongodb_url.push(c),
                                            ActiveEngine::Neo4j => state.conn_fields.neo4j_url.push(c),
                                            ActiveEngine::LocalJson => state.conn_fields.json_path.push(c),
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                match state.connection_mode {
                                    ConnectionMode::Profiles => {}
                                    ConnectionMode::Form => {
                                        match state.active_form_field {
                                            FormField::Host => { state.form_fields.host.pop(); }
                                            FormField::Port => { state.form_fields.port.pop(); }
                                            FormField::User => { state.form_fields.user.pop(); }
                                            FormField::Pass => { state.form_fields.pass.pop(); }
                                            FormField::Db => { state.form_fields.db_or_path.pop(); }
                                        }
                                    }
                                    ConnectionMode::RawUrl => {
                                        match state.active_engine {
                                            ActiveEngine::MariaDb => { state.conn_fields.mysql_url.pop(); }
                                            ActiveEngine::PostgreSql => { state.conn_fields.postgres_url.pop(); }
                                            ActiveEngine::Sqlite => { state.conn_fields.sqlite_path.pop(); }
                                            ActiveEngine::MongoDb => { state.conn_fields.mongodb_url.pop(); }
                                            ActiveEngine::Neo4j => { state.conn_fields.neo4j_url.pop(); }
                                            ActiveEngine::LocalJson => { state.conn_fields.json_path.pop(); }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::Sidebar => {
                        match key.code {
                            KeyCode::Up => {
                                if state.show_db_list {
                                    if let Some(idx) = state.selected_db_idx {
                                        if idx > 0 {
                                            state.selected_db_idx = Some(idx - 1);
                                        }
                                    }
                                } else {
                                    if let Some(idx) = state.selected_table_idx {
                                        if idx > 0 {
                                            state.selected_table_idx = Some(idx - 1);
                                        }
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if state.show_db_list {
                                    if let Some(idx) = state.selected_db_idx {
                                        if idx < state.databases.len() - 1 {
                                            state.selected_db_idx = Some(idx + 1);
                                        }
                                    }
                                } else {
                                    if let Some(idx) = state.selected_table_idx {
                                        if idx < state.tables.len() - 1 {
                                            state.selected_table_idx = Some(idx + 1);
                                        }
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                if state.show_db_list {
                                    state.show_db_list = false;
                                } else {
                                    // Disconnect and go back to selection screen
                                    state.connected = false;
                                    state.active_pane = ActivePane::EngineSelector;
                                    state.conn_status_msg = "Ready to connect".to_string();
                                }
                            }
                            KeyCode::Char('d') => {
                                state.show_db_list = !state.show_db_list;
                                if state.show_db_list {
                                    state.connecting = true;
                                    state.conn_status_msg = "Loading databases...".to_string();
                                    let _ = app_tx.send(DbRequest::LoadDatabases).await;
                                }
                            }
                            KeyCode::Enter => {
                                if state.show_db_list {
                                    if let Some(idx) = state.selected_db_idx {
                                        if idx < state.databases.len() {
                                            let db_name = state.databases[idx].clone();
                                            state.connecting = true;
                                            state.conn_status_msg = format!("Switching database to {}...", db_name);
                                            state.show_db_list = false;
                                            let _ = app_tx.send(DbRequest::SelectDatabase(db_name)).await;
                                        }
                                    }
                                } else {
                                    // Trigger automated query for the selected table
                                    if let Some(idx) = state.selected_table_idx {
                                        let table = &state.tables[idx];
                                        state.active_table_name = Some(table.clone());
                                        state.exploration_history.clear();
                                        let query = match state.active_engine {
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
                                        state.sql_console_input = query.clone();
                                        state.sql_cursor_pos = query.len();
                                        state.connecting = true;
                                        let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                                        let _ = app_tx.send(DbRequest::LoadMetadata { table: table.clone() }).await;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::SqlConsole => {
                        match key.code {
                            KeyCode::Char(c) => {
                                state.sql_console_input.insert(state.sql_cursor_pos, c);
                                state.sql_cursor_pos += 1;
                            }
                            KeyCode::Left => {
                                if state.sql_cursor_pos > 0 {
                                    state.sql_cursor_pos -= 1;
                                }
                            }
                            KeyCode::Right => {
                                if state.sql_cursor_pos < state.sql_console_input.len() {
                                    state.sql_cursor_pos += 1;
                                }
                            }
                            KeyCode::Backspace => {
                                if state.sql_cursor_pos > 0 {
                                    state.sql_cursor_pos -= 1;
                                    state.sql_console_input.remove(state.sql_cursor_pos);
                                }
                            }
                            KeyCode::Esc => {
                                state.active_pane = ActivePane::Sidebar;
                            }
                            KeyCode::Enter => {
                                // Run user console query
                                if !state.sql_console_input.trim().is_empty() {
                                    state.connecting = true;
                                    state.exploration_history.clear();
                                    let query = state.sql_console_input.clone();
                                    if let Some(table) = extract_active_table(&query, state.active_engine) {
                                        state.active_table_name = Some(table.clone());
                                        let _ = app_tx.send(DbRequest::LoadMetadata { table }).await;
                                    } else {
                                        state.active_table_name = None;
                                        state.primary_key = None;
                                        state.relationships.clear();
                                        state.related_headers.clear();
                                        state.related_rows.clear();
                                    }
                                    let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::QueryResults => {
                        if state.show_delete_confirm {
                            match key.code {
                                KeyCode::Enter => {
                                    if let Some(query) = state.compile_delete_statement() {
                                        if let Some(ref tbl) = state.active_table_name {
                                            let reload = format!("SELECT * FROM {} LIMIT 50;", tbl);
                                            state.mutation_in_progress = Some(reload);
                                            state.connecting = true;
                                            let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                                        }
                                    }
                                    state.show_delete_confirm = false;
                                }
                                KeyCode::Esc => {
                                    state.show_delete_confirm = false;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        if state.bi_mode_enabled {
                            match key.code {
                                KeyCode::Up => {
                                    state.move_bi_selector(true);
                                }
                                KeyCode::Down => {
                                    state.move_bi_selector(false);
                                }
                                KeyCode::Left => {
                                    state.cycle_bi_selector(false);
                                }
                                KeyCode::Right => {
                                    state.cycle_bi_selector(true);
                                }
                                KeyCode::Char('x') | KeyCode::Char('X') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                                    state.recalculate_pivot();
                                }
                                KeyCode::Char(c) => {
                                    if state.pivot_state.active_selector_idx == 6 {
                                        state.pivot_state.filter_text_input.push(c);
                                        if state.pivot_state.config.auto_recalc {
                                            state.pivot_state.config.filter_val = state.pivot_state.filter_text_input.clone();
                                            state.recalculate_pivot();
                                        }
                                    }
                                }
                                KeyCode::Backspace => {
                                    if state.pivot_state.active_selector_idx == 6 {
                                        state.pivot_state.filter_text_input.pop();
                                        if state.pivot_state.config.auto_recalc {
                                            state.pivot_state.config.filter_val = state.pivot_state.filter_text_input.clone();
                                            state.recalculate_pivot();
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    if state.pivot_state.active_selector_idx == 6 {
                                        state.pivot_state.config.filter_val = state.pivot_state.filter_text_input.clone();
                                        state.recalculate_pivot();
                                    } else {
                                        state.recalculate_pivot();
                                    }
                                }
                                KeyCode::Esc => {
                                    state.bi_mode_enabled = false;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // Describe/schema overlay: any of i/Esc/Enter closes it.
                        if state.show_describe {
                            match key.code {
                                KeyCode::Char('i') | KeyCode::Char('I') | KeyCode::Esc | KeyCode::Enter => {
                                    state.show_describe = false;
                                }
                                _ => {}
                            }
                            continue;
                        }

                        // In-grid searcher typing mode.
                        if state.search_active {
                            match key.code {
                                KeyCode::Char(c) => {
                                    state.search_query.push(c);
                                    state.clamp_selection_to_visible();
                                }
                                KeyCode::Backspace => {
                                    state.search_query.pop();
                                    state.clamp_selection_to_visible();
                                }
                                KeyCode::Enter => {
                                    // Keep the filter but leave typing mode.
                                    state.search_active = false;
                                }
                                KeyCode::Esc => {
                                    // Clear the filter entirely.
                                    state.search_active = false;
                                    state.search_query.clear();
                                    state.clamp_selection_to_visible();
                                }
                                _ => {}
                            }
                            if let Some(req) = load_related_data_if_needed(&state) {
                                let _ = app_tx.send(req).await;
                            }
                            continue;
                        }

                        let is_dbf = state.active_engine == ActiveEngine::LocalJson && state.conn_fields.json_path.ends_with(".dbf");
                        let is_document_view = (state.active_engine == ActiveEngine::MongoDb || state.active_engine == ActiveEngine::LocalJson) && !is_dbf;
                        match key.code {
                            KeyCode::Char('/') => {
                                if !is_document_view {
                                    state.search_active = true;
                                }
                            }
                            KeyCode::Char('i') | KeyCode::Char('I') => {
                                state.toggle_describe();
                            }
                            KeyCode::Up => {
                                if is_document_view {
                                    if let Some(idx) = state.selected_tree_row_idx {
                                        if idx > 0 {
                                            state.selected_tree_row_idx = Some(idx - 1);
                                        }
                                    }
                                } else if state.step_visible_selection(false) {
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if is_document_view {
                                    if let Some(idx) = state.selected_tree_row_idx {
                                        if idx < state.flat_tree_rows.len() - 1 {
                                            state.selected_tree_row_idx = Some(idx + 1);
                                        }
                                    }
                                } else if state.step_visible_selection(true) {
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                // Toggle tree item expansion if document view
                                if (state.active_engine == ActiveEngine::MongoDb || state.active_engine == ActiveEngine::LocalJson) && !is_dbf {
                                    state.toggle_selected_tree_item();
                                }
                            }
                            KeyCode::Char('e') => {
                                state.open_edit_row_modal();
                            }
                            KeyCode::Char('a') => {
                                state.open_add_row_modal();
                            }
                            KeyCode::Char('d') => {
                                if let Some(idx) = state.selected_row_idx {
                                    if idx < state.result_rows.len() {
                                        state.show_delete_confirm = true;
                                        if let Some(ref pk_col) = state.primary_key {
                                            if let Some(pos) = state.result_headers.iter().position(|x| x.to_lowercase() == pk_col.to_lowercase()) {
                                                state.selected_row_pk_val = state.result_rows[idx].get(pos).cloned();
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::F(6) => {
                                // Toggle BI charting mode if applicable
                                if state.bi_chartable {
                                    state.bi_mode_enabled = !state.bi_mode_enabled;
                                }
                            }
                            KeyCode::F(7) => {
                                state.show_related_split = !state.show_related_split;
                                if state.show_related_split {
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            KeyCode::Left => {
                                if state.col_scroll_offset > 0 {
                                    state.col_scroll_offset -= 1;
                                }
                            }
                            KeyCode::Right => {
                                let num_cols = state.result_headers.len();
                                if num_cols > 0 && state.col_scroll_offset + 1 < num_cols {
                                    state.col_scroll_offset += 1;
                                }
                            }
                            KeyCode::Esc => {
                                state.active_pane = ActivePane::SqlConsole;
                            }
                            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Char('B') => {
                                if !state.exploration_history.is_empty() {
                                    if let Some(prev) = state.exploration_history.pop() {
                                        state.active_table_name = prev.table_name.clone();
                                        state.sql_console_input = prev.query.clone();
                                        state.sql_cursor_pos = prev.query.len();
                                        state.selected_row_idx = prev.selected_row_idx;
                                        state.active_relationship_idx = prev.active_relationship_idx;
                                        state.show_related_split = prev.show_related_split;
                                        state.connecting = true;
                                        let _ = app_tx.send(DbRequest::ExecuteQuery(prev.query)).await;
                                        if let Some(ref tbl) = prev.table_name {
                                            let _ = app_tx.send(DbRequest::LoadMetadata { table: tbl.clone() }).await;
                                        }
                                        continue;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::RelatedDataList => {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('w') | KeyCode::PageUp => {
                                if !state.relationships.is_empty() {
                                    if state.active_relationship_idx > 0 {
                                        state.active_relationship_idx -= 1;
                                    } else {
                                        state.active_relationship_idx = state.relationships.len() - 1;
                                    }
                                    state.related_selected_row_idx = None;
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('s') | KeyCode::PageDown => {
                                if !state.relationships.is_empty() {
                                    if state.active_relationship_idx < state.relationships.len() - 1 {
                                        state.active_relationship_idx += 1;
                                    } else {
                                        state.active_relationship_idx = 0;
                                    }
                                    state.related_selected_row_idx = None;
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            KeyCode::Right | KeyCode::Enter => {
                                if !state.related_rows.is_empty() {
                                    state.active_pane = ActivePane::RelatedDataGrid;
                                }
                            }
                            KeyCode::Esc => {
                                state.active_pane = ActivePane::QueryResults;
                            }
                            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Char('B') => {
                                if !state.exploration_history.is_empty() {
                                    if let Some(prev) = state.exploration_history.pop() {
                                        state.active_table_name = prev.table_name.clone();
                                        state.sql_console_input = prev.query.clone();
                                        state.sql_cursor_pos = prev.query.len();
                                        state.selected_row_idx = prev.selected_row_idx;
                                        state.active_relationship_idx = prev.active_relationship_idx;
                                        state.show_related_split = prev.show_related_split;
                                        state.connecting = true;
                                        let _ = app_tx.send(DbRequest::ExecuteQuery(prev.query)).await;
                                        if let Some(ref tbl) = prev.table_name {
                                            let _ = app_tx.send(DbRequest::LoadMetadata { table: tbl.clone() }).await;
                                        }
                                        continue;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::RelatedDataGrid => {
                        match key.code {
                            KeyCode::Left => {
                                if state.related_col_scroll_offset > 0 {
                                    state.related_col_scroll_offset -= 1;
                                } else {
                                    state.active_pane = ActivePane::RelatedDataList;
                                }
                            }
                            KeyCode::Right => {
                                let num_cols = state.related_headers.len();
                                if num_cols > 0 && state.related_col_scroll_offset + 1 < num_cols {
                                    state.related_col_scroll_offset += 1;
                                }
                            }
                            KeyCode::Up | KeyCode::Char('w') | KeyCode::PageUp => {
                                if !state.related_rows.is_empty() {
                                    let current = state.related_selected_row_idx.unwrap_or(0);
                                    if current > 0 {
                                        state.related_selected_row_idx = Some(current - 1);
                                    } else {
                                        state.related_selected_row_idx = Some(state.related_rows.len() - 1);
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('s') | KeyCode::PageDown => {
                                if !state.related_rows.is_empty() {
                                    let current = state.related_selected_row_idx.unwrap_or(0);
                                    if current < state.related_rows.len() - 1 {
                                        state.related_selected_row_idx = Some(current + 1);
                                    } else {
                                        state.related_selected_row_idx = Some(0);
                                    }
                                }
                            }
                            KeyCode::Esc => {
                                state.active_pane = ActivePane::RelatedDataList;
                            }
                            KeyCode::Enter | KeyCode::Char('g') | KeyCode::Char('G') => {
                                if state.related_selected_row_idx.is_some() {
                                    if state.active_relationship_idx < state.relationships.len() {
                                        let rel = state.relationships[state.active_relationship_idx].clone();
                                        
                                        // Find the active row value of the relationship from the main table
                                        if let Some(row_idx) = state.selected_row_idx {
                                            if let Some(row) = state.result_rows.get(row_idx) {
                                                if let Some(col_pos) = state.result_headers.iter().position(|c| c.to_lowercase() == rel.active_col.to_lowercase()) {
                                                    if let Some(active_val) = row.get(col_pos) {
                                                        // Push to exploration history stack
                                                        let current_state = ExplorationState {
                                                            table_name: state.active_table_name.clone(),
                                                            query: state.sql_console_input.clone(),
                                                            selected_row_idx: state.selected_row_idx,
                                                            active_relationship_idx: state.active_relationship_idx,
                                                            show_related_split: state.show_related_split,
                                                        };
                                                        state.exploration_history.push(current_state);
                                                        
                                                        // Construct query for the target table
                                                        let escaped_val = active_val.replace('\'', "''");
                                                        let query = format!(
                                                            "SELECT * FROM {} WHERE {} = '{}';",
                                                            rel.target_table,
                                                            rel.target_col,
                                                            escaped_val
                                                        );
                                                        
                                                        // Update app state for the new query
                                                        state.active_table_name = Some(rel.target_table.clone());
                                                        state.sql_console_input = query.clone();
                                                        state.sql_cursor_pos = query.len();
                                                        state.selected_row_idx = Some(0);
                                                        state.active_pane = ActivePane::QueryResults;
                                                        state.connecting = true;
                                                        
                                                        // Execute and load metadata
                                                        let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                                                        let _ = app_tx.send(DbRequest::LoadMetadata { table: rel.target_table.clone() }).await;
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Backspace | KeyCode::Char('b') | KeyCode::Char('B') => {
                                if !state.exploration_history.is_empty() {
                                    if let Some(prev) = state.exploration_history.pop() {
                                        state.active_table_name = prev.table_name.clone();
                                        state.sql_console_input = prev.query.clone();
                                        state.sql_cursor_pos = prev.query.len();
                                        state.selected_row_idx = prev.selected_row_idx;
                                        state.active_relationship_idx = prev.active_relationship_idx;
                                        state.show_related_split = prev.show_related_split;
                                        state.connecting = true;
                                        let _ = app_tx.send(DbRequest::ExecuteQuery(prev.query)).await;
                                        if let Some(ref tbl) = prev.table_name {
                                            let _ = app_tx.send(DbRequest::LoadMetadata { table: tbl.clone() }).await;
                                        }
                                        continue;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ActivePane::ModalEditor => {
                        match key.code {
                            KeyCode::Esc => {
                                state.show_edit_modal = false;
                                state.show_add_modal = false;
                                state.active_pane = ActivePane::QueryResults;
                            }
                            KeyCode::Up => {
                                if !state.modal_fields.is_empty() {
                                    if state.active_modal_field_idx > 0 {
                                        state.active_modal_field_idx -= 1;
                                    } else {
                                        state.active_modal_field_idx = state.modal_fields.len() - 1;
                                    }
                                }
                            }
                            KeyCode::Down => {
                                if !state.modal_fields.is_empty() {
                                    if state.active_modal_field_idx < state.modal_fields.len() - 1 {
                                        state.active_modal_field_idx += 1;
                                    } else {
                                        state.active_modal_field_idx = 0;
                                    }
                                }
                            }
                            KeyCode::Char(c) => {
                                if !state.modal_fields.is_empty() {
                                    state.modal_fields[state.active_modal_field_idx].1.push(c);
                                }
                            }
                            KeyCode::Backspace => {
                                if !state.modal_fields.is_empty() {
                                    state.modal_fields[state.active_modal_field_idx].1.pop();
                                }
                            }
                            KeyCode::Enter => {
                                if !state.modal_fields.is_empty() {
                                    let mut query_opt = None;
                                    if state.show_edit_modal {
                                        query_opt = state.compile_update_statement();
                                    } else if state.show_add_modal {
                                        query_opt = state.compile_insert_statement();
                                    }
                                    if let Some(query) = query_opt {
                                        if let Some(ref tbl) = state.active_table_name {
                                            let reload = format!("SELECT * FROM {} LIMIT 50;", tbl);
                                            state.mutation_in_progress = Some(reload);
                                            state.connecting = true;
                                            let _ = app_tx.send(DbRequest::ExecuteQuery(query)).await;
                                        }
                                    }
                                    state.show_edit_modal = false;
                                    state.show_add_modal = false;
                                    state.active_pane = ActivePane::QueryResults;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                }
                Event::Mouse(mouse_event) => {
                    use crossterm::event::MouseEventKind;
                    // Mouse wheel scrolls the focused list/grid.
                    if matches!(mouse_event.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp) {
                        let forward = mouse_event.kind == MouseEventKind::ScrollDown;
                        let is_dbf = state.active_engine == ActiveEngine::LocalJson && state.conn_fields.json_path.ends_with(".dbf");
                        let is_document_view = (state.active_engine == ActiveEngine::MongoDb || state.active_engine == ActiveEngine::LocalJson) && !is_dbf;
                        match state.active_pane {
                            ActivePane::Sidebar => {
                                let len = if state.show_db_list { state.databases.len() } else { state.tables.len() };
                                let cur = if state.show_db_list { &mut state.selected_db_idx } else { &mut state.selected_table_idx };
                                if let Some(idx) = cur {
                                    if forward { if *idx + 1 < len { *cur = Some(*idx + 1); } }
                                    else if *idx > 0 { *cur = Some(*idx - 1); }
                                }
                            }
                            ActivePane::QueryResults => {
                                if is_document_view {
                                    if let Some(idx) = state.selected_tree_row_idx {
                                        if forward { if idx + 1 < state.flat_tree_rows.len() { state.selected_tree_row_idx = Some(idx + 1); } }
                                        else if idx > 0 { state.selected_tree_row_idx = Some(idx - 1); }
                                    }
                                } else if state.step_visible_selection(forward) {
                                    if let Some(req) = load_related_data_if_needed(&state) {
                                        let _ = app_tx.send(req).await;
                                    }
                                }
                            }
                            ActivePane::RelatedDataGrid => {
                                if !state.related_rows.is_empty() {
                                    let cur = state.related_selected_row_idx.unwrap_or(0);
                                    if forward { if cur + 1 < state.related_rows.len() { state.related_selected_row_idx = Some(cur + 1); } }
                                    else if cur > 0 { state.related_selected_row_idx = Some(cur - 1); }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if mouse_event.kind == MouseEventKind::Down(crossterm::event::MouseButton::Left) {
                        let col = mouse_event.column;
                        let row = mouse_event.row;
                        if let Some(req) = state.handle_mouse_click(col, row) {
                            match &req {
                                DbRequest::SelectDatabase(_) => {
                                    let _ = app_tx.send(req).await;
                                }
                                DbRequest::ExecuteQuery(_) => {
                                    let _ = app_tx.send(req).await;
                                    if let Some(ref table) = state.active_table_name {
                                        let _ = app_tx.send(DbRequest::LoadMetadata { table: table.clone() }).await;
                                    }
                                }
                                _ => {
                                    let _ = app_tx.send(req).await;
                                }
                            }
                        } else {
                            if let Some(req) = load_related_data_if_needed(&state) {
                                let _ = app_tx.send(req).await;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Request loading of tables when newly connected (but list is empty)
        if state.connected && state.tables.is_empty() && !state.connecting && !state.show_db_list {
            state.connecting = true;
            let _ = app_tx.send(DbRequest::LoadTables).await;
        }
    }

    // Restore terminal state
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, event::DisableMouseCapture)?;
    Ok(())
}
