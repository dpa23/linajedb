use crate::app::{ActiveEngine, ActivePane, AppState, BiChartType};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Sparkline, Table, Wrap},
    Frame,
};

#[derive(Debug, Clone)]
pub struct BiBarData {
    pub label: String,
    pub value: u64,
}

// Charcoal / Slate Modern Palette
pub struct Theme {
    pub border_active: Color,
    pub border_inactive: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub header_fg: Color,
    pub accent: Color,
    pub danger: Color,
    pub success: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border_active: Color::Rgb(137, 180, 250),   // Pastel Blue
            border_inactive: Color::Rgb(88, 91, 112),   // Dark Grey
            text_primary: Color::Rgb(220, 224, 232),    // Off-white
            text_secondary: Color::Rgb(166, 173, 200),  // Muted gray
            header_fg: Color::Rgb(249, 226, 175),       // Soft gold
            accent: Color::Rgb(137, 220, 235),          // Teal
            danger: Color::Rgb(243, 139, 168),          // Red
            success: Color::Rgb(166, 227, 161),         // Green
        }
    }
}

pub fn get_pane_block(title: &str, is_focused: bool, theme: &Theme) -> Block<'static> {
    if is_focused {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_active))
            .title(format!(" [{}] ", title))
            .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border_inactive))
            .title(format!(" {} ", title))
            .title_style(Style::default().fg(theme.text_secondary))
    }
}

pub fn get_centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn draw_ui(f: &mut Frame, state: &mut AppState) {
    let size = f.size();
    let theme = Theme::default();

    // Responsive warning if terminal is too small
    if size.width < 80 || size.height < 24 {
        let warning_text = format!(
            "Terminal size too small!\n\nMinimum required: 80x24\nCurrent size: {}x{}\n\nPlease resize your terminal window.",
            size.width, size.height
        );
        let paragraph = Paragraph::new(warning_text)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.danger)))
            .alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(theme.header_fg));
        f.render_widget(paragraph, size);
        return;
    }

    // Connect Screen Overlay (if not connected)
    if !state.connected {
        draw_connection_screen(f, size, state, &theme);
        return;
    }

    // Normal Layout splits: Header (tabs), Main workspace (sidebar + data view), Footer (console + hotkeys)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header Tab Bar
            Constraint::Min(10),   // Main View (Tables list + Data panel)
            Constraint::Length(3), // SQL input console
            Constraint::Length(1), // Footer status bar
        ])
        .split(size);

    draw_header_tabs(f, chunks[0], state, &theme);
    draw_workspace(f, chunks[1], state, &theme);
    draw_sql_console(f, chunks[2], state, &theme);
    draw_footer_status(f, chunks[3], state, &theme);

    // Draw modals if active
    if state.show_edit_modal || state.show_add_modal {
        draw_row_editor_modal(f, size, state, &theme);
    } else if state.show_delete_confirm {
        draw_delete_confirm_modal(f, size, state, &theme);
    }
}

fn draw_connection_screen(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    // Render back wall of selector screen
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_inactive))
        .title(" db-tui - Dynamic Access Database Client ")
        .title_style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD));
    
    let welcome_msg = Paragraph::new(
        "\n  Welcome to db-tui - Local Multi-Engine Terminal Client\n\n  Press [F2] for Profiles, [F3] for Manual Form, [F4] for Raw Connection URL.\n  Use Left/Right to change engines, Up/Down to navigate fields, and Enter to connect.\n"
    ).block(outer_block).style(Style::default().fg(theme.text_secondary));
    f.render_widget(welcome_msg, area);

    // Draw centered connecting modal dialog (larger size to fit forms)
    let modal_area = get_centered_rect(85, 75, area);
    f.render_widget(Clear, modal_area);

    let is_focused = state.active_pane == ActivePane::EngineSelector;
    let modal_block = get_pane_block(" CONNECTION SETUP ", is_focused, theme);
    f.render_widget(modal_block.clone(), modal_area);

    let inner_area = modal_block.inner(modal_area);

    // Split inner area horizontally: Left (25% for Engine List), Right (75% for credentials/setup)
    let setup_splits = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Engines List
            Constraint::Percentage(75), // Connection details form
        ])
        .split(inner_area);

    // 1. Draw Left Pane (Engines List)
    let engine_block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme.border_inactive))
        .title(" Database Engines ");
    
    let items: Vec<ListItem> = vec![
        ActiveEngine::MariaDb,
        ActiveEngine::PostgreSql,
        ActiveEngine::Sqlite,
        ActiveEngine::MongoDb,
        ActiveEngine::Neo4j,
        ActiveEngine::LocalJson,
    ]
    .into_iter()
    .map(|eng| {
        let active = state.active_engine == eng;
        let style = if active {
            Style::default().bg(theme.border_active).fg(Color::Black).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_primary)
        };
        ListItem::new(format!(" {} {}", if active { "▶" } else { " " }, eng.name())).style(style)
    })
    .collect();
    let engine_list = List::new(items).block(engine_block);
    f.render_widget(engine_list, setup_splits[0]);

    // 2. Draw Right Pane (Credentials Form or Profiles list)
    // Splits: Modes Tab bar (Length 3), Form Content (Min 5), Status Line (Length 1)
    let right_splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Modes tabs
            Constraint::Min(5),    // Mode content
            Constraint::Length(1), // Status line
        ])
        .split(setup_splits[1]);

    // Draw Modes Tabs
    let mode_tabs_titles = vec!["[F2] Discovered Profiles", "[F3] Manual Form", "[F4] Raw URL"];
    let active_mode_idx = match state.connection_mode {
        crate::app::ConnectionMode::Profiles => 0,
        crate::app::ConnectionMode::Form => 1,
        crate::app::ConnectionMode::RawUrl => 2,
    };
    let mode_tabs = ratatui::widgets::Tabs::new(mode_tabs_titles)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(theme.border_inactive)))
        .select(active_mode_idx)
        .style(Style::default().fg(theme.text_secondary))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    f.render_widget(mode_tabs, right_splits[0]);

    // Draw Mode Content
    let content_area = right_splits[1];
    match state.connection_mode {
        crate::app::ConnectionMode::Profiles => {
            // Filter profiles matching current active engine
            let filtered_profiles: Vec<(usize, &crate::app::ConnectionProfile)> = state.profiles
                .iter()
                .enumerate()
                .filter(|(_, p)| p.engine == state.active_engine)
                .collect();

            if filtered_profiles.is_empty() {
                let empty_para = Paragraph::new("\n  No discovered profiles found for this engine.\n  Use F3 to enter credentials manually.")
                    .style(Style::default().fg(theme.text_secondary));
                f.render_widget(empty_para, content_area);
            } else {
                let items: Vec<ListItem> = filtered_profiles
                    .iter()
                    .enumerate()
                    .map(|(_, (original_idx, profile))| {
                        let is_selected = state.selected_profile_idx == *original_idx;
                        let style = if is_selected {
                            Style::default().bg(Color::Rgb(49, 50, 68)).fg(theme.border_active).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.text_primary)
                        };
                        ListItem::new(format!("  {}  {}", if is_selected { "●" } else { "○" }, profile.name)).style(style)
                    })
                    .collect();
                let profiles_list = List::new(items).block(Block::default().title(" Select Profile "));
                f.render_widget(profiles_list, content_area);
            }
        }
        crate::app::ConnectionMode::Form => {
            // Form has 5 input fields
            let form_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2), // Host
                    Constraint::Length(2), // Port
                    Constraint::Length(2), // User
                    Constraint::Length(2), // Pass
                    Constraint::Length(2), // Db/Path
                ])
                .split(content_area);

            let fields = [
                (crate::app::FormField::Host, &state.form_fields.host),
                (crate::app::FormField::Port, &state.form_fields.port),
                (crate::app::FormField::User, &state.form_fields.user),
                (crate::app::FormField::Pass, &state.form_fields.pass),
                (crate::app::FormField::Db, &state.form_fields.db_or_path),
            ];

            for (idx, (field, value)) in fields.iter().enumerate() {
                let is_active = state.active_form_field == *field;
                let text_style = if is_active {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_secondary)
                };

                let label_text = format!(" {} ", field.label(state.active_engine));
                let value_display = if *field == crate::app::FormField::Pass {
                    "*".repeat(value.len())
                } else {
                    (*value).clone()
                };

                let input_line = Line::from(vec![
                    Span::styled(label_text, text_style),
                    Span::styled(value_display, Style::default().fg(theme.text_primary)),
                ]);

                let block_style = if is_active {
                    Style::default().fg(theme.border_active)
                } else {
                    Style::default().fg(theme.border_inactive)
                };
                let row_block = Block::default().borders(Borders::BOTTOM).border_style(block_style);
                f.render_widget(Paragraph::new(input_line).block(row_block), form_layout[idx]);

                if is_focused && is_active && !state.connecting {
                    let label_len = field.label(state.active_engine).len() as u16;
                    let cursor_x = form_layout[idx].x + label_len + value.len() as u16 + 2;
                    let cursor_y = form_layout[idx].y;
                    f.set_cursor(cursor_x, cursor_y);
                }
            }
        }
        crate::app::ConnectionMode::RawUrl => {
            let conn_val = match state.active_engine {
                ActiveEngine::MariaDb => &state.conn_fields.mysql_url,
                ActiveEngine::PostgreSql => &state.conn_fields.postgres_url,
                ActiveEngine::Sqlite => &state.conn_fields.sqlite_path,
                ActiveEngine::MongoDb => &state.conn_fields.mongodb_url,
                ActiveEngine::Neo4j => &state.conn_fields.neo4j_url,
                ActiveEngine::LocalJson => &state.conn_fields.json_path,
            };

            let conn_label = match state.active_engine {
                ActiveEngine::MariaDb => "Raw MySQL Connection URL:",
                ActiveEngine::PostgreSql => "Raw PostgreSQL Connection URL:",
                ActiveEngine::Sqlite => "SQLite file path:",
                ActiveEngine::MongoDb => "Raw MongoDB Connection URL:",
                ActiveEngine::Neo4j => "Raw Neo4j Bolt URL:",
                ActiveEngine::LocalJson => "JSON file path:",
            };

            let raw_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Spacer
                    Constraint::Length(1), // Label
                    Constraint::Length(3), // Input
                ])
                .split(content_area);

            f.render_widget(Paragraph::new(conn_label).style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)), raw_layout[1]);
            let input_block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.border_inactive));
            f.render_widget(Paragraph::new(conn_val.as_str()).block(input_block).style(Style::default().fg(theme.text_primary)), raw_layout[2]);

            if is_focused && !state.connecting {
                let cursor_x = raw_layout[2].x + conn_val.len() as u16 + 1;
                let cursor_y = raw_layout[2].y + 1;
                f.set_cursor(cursor_x, cursor_y);
            }
        }
    }

    // Draw Status message
    let status_style = if state.connecting {
        Style::default().fg(theme.header_fg).add_modifier(Modifier::SLOW_BLINK)
    } else if state.conn_status_msg.starts_with("Error") {
        Style::default().fg(theme.danger)
    } else {
        Style::default().fg(theme.success)
    };
    
    let status_text = if state.connecting {
        "Connecting to database... Please wait."
    } else {
        &state.conn_status_msg
    };
    f.render_widget(Paragraph::new(status_text).style(status_style), right_splits[2]);
}

fn draw_header_tabs(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    state.rect_header_tabs = Some(area);

    let tabs_titles = vec!["[1] Tables/Schemas List", "[2] SQL Console", "[3] Visual BI Dash"];
    let is_focused = state.active_pane == ActivePane::EngineSelector;
    let border_color = if is_focused { theme.border_active } else { theme.border_inactive };

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(border_color))
        .title(format!(" db-tui - {} Client ", state.active_engine.name()))
        .title_style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD));

    let active_idx = match state.active_pane {
        ActivePane::Sidebar => 0,
        ActivePane::SqlConsole => 1,
        ActivePane::QueryResults => if state.bi_mode_enabled { 2 } else { 1 },
        ActivePane::RelatedDataList | ActivePane::RelatedDataGrid => if state.bi_mode_enabled { 2 } else { 1 },
        ActivePane::ModalEditor => if state.bi_mode_enabled { 2 } else { 1 },
        ActivePane::EngineSelector => 0,
    };

    let tabs = ratatui::widgets::Tabs::new(tabs_titles)
        .block(block)
        .select(active_idx)
        .style(Style::default().fg(theme.text_secondary))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::UNDERLINED),
        );

    f.render_widget(tabs, area);
}

fn draw_workspace(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let workspace_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Sidebar list
            Constraint::Percentage(75), // Details / Grid View
        ])
        .split(area);

    draw_sidebar(f, workspace_layout[0], state, theme);
    state.rect_sidebar = Some(workspace_layout[0]);

    if state.bi_mode_enabled && state.bi_chartable {
        state.rect_related_split = None;
        state.rect_data_view = None;
        draw_bi_dashboard(f, workspace_layout[1], state, theme);
    } else {
        draw_data_details(f, workspace_layout[1], state, theme);
    }
}

fn draw_data_details(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let is_focused = state.active_pane == ActivePane::QueryResults;
    let block = get_pane_block(" DATA VIEW ", is_focused, theme);

    let is_relational = state.active_engine == ActiveEngine::MariaDb
        || state.active_engine == ActiveEngine::PostgreSql
        || state.active_engine == ActiveEngine::Sqlite;

    if !state.show_related_split || !is_relational || state.relationships.is_empty() {
        state.rect_related_split = None;
        state.rect_data_view = Some(area);
        draw_main_details_grid(f, area, state, theme, block);
    } else {
        let splits = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(60), // Main table grid
                Constraint::Percentage(40), // Parent/Child records
            ])
            .split(area);

        state.rect_data_view = Some(splits[0]);
        state.rect_related_split = Some(splits[1]);

        draw_main_details_grid(f, splits[0], state, theme, block);
        draw_related_data_split(f, splits[1], state, theme);
    }
}

fn draw_sidebar(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let is_focused = state.active_pane == ActivePane::Sidebar;

    if state.show_db_list {
        let block = get_pane_block(" DATABASES/SCHEMAS (Esc/d: Back) ", is_focused, theme);
        let items: Vec<ListItem> = state
            .databases
            .iter()
            .enumerate()
            .map(|(idx, db)| {
                let active = state.selected_db_idx == Some(idx);
                let style = if active {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_primary)
                };
                ListItem::new(format!("  {} {}", if active { "🔘" } else { "  " }, db)).style(style)
            })
            .collect();
            
        let list = List::new(items).block(block);
        let mut list_state = ListState::default();
        list_state.select(state.selected_db_idx);
        f.render_stateful_widget(list, area, &mut list_state);
    } else {
        let sidebar_title = match state.active_engine {
            ActiveEngine::MongoDb => " COLLECTIONS (d: Switch DB) ",
            ActiveEngine::Neo4j => " LABELS (d: Switch DB) ",
            _ => " TABLES (d: Switch DB) ",
        };
        let block = get_pane_block(sidebar_title, is_focused, theme);

        let items: Vec<ListItem> = state
            .tables
            .iter()
            .enumerate()
            .map(|(idx, table)| {
                let active = state.selected_table_idx == Some(idx);
                let style = if active {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text_primary)
                };
                ListItem::new(format!("  {} {}", if active { "🔘" } else { "  " }, table)).style(style)
            })
            .collect();

        let list = List::new(items).block(block);
        let mut list_state = ListState::default();
        list_state.select(state.selected_table_idx);
        f.render_stateful_widget(list, area, &mut list_state);
    }
}

fn draw_main_details_grid(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme, block: Block<'static>) {
    // If document engine (MongoDB/JSON), render as tree view
    let is_dbf = state.active_engine == ActiveEngine::LocalJson && state.conn_fields.json_path.ends_with(".dbf");
    if (state.active_engine == ActiveEngine::MongoDb || state.active_engine == ActiveEngine::LocalJson) && !is_dbf {
        if state.flat_tree_rows.is_empty() {
            let empty_msg = Paragraph::new("\n  Query returned 0 documents / Execute a filter to list records.")
                .block(block)
                .style(Style::default().fg(theme.text_secondary));
            f.render_widget(empty_msg, area);
            return;
        }

        let items: Vec<ListItem> = state
            .flat_tree_rows
            .iter()
            .enumerate()
            .map(|(idx, row)| {
                let is_selected = state.selected_tree_row_idx == Some(idx);
                let mut style = Style::default().fg(theme.text_primary);
                if is_selected {
                    style = style.bg(Color::Rgb(49, 50, 68)).fg(theme.border_active).add_modifier(Modifier::BOLD);
                }
                
                let line_str = &row.display_text;
                let spans = if line_str.contains(':') {
                    let parts: Vec<&str> = line_str.splitn(2, ':').collect();
                    vec![
                        Span::styled(parts[0].to_string(), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                        Span::raw(":"),
                        Span::styled(parts[1].to_string(), Style::default().fg(theme.text_primary)),
                    ]
                } else {
                    vec![Span::raw(line_str.clone())]
                };

                ListItem::new(Line::from(spans)).style(style)
            })
            .collect();

        let list = List::new(items).block(block);
        let mut list_state = ListState::default();
        list_state.select(state.selected_tree_row_idx);
        f.render_stateful_widget(list, area, &mut list_state);
    } else {
        // Relational Grid/Table View
        if state.result_rows.is_empty() {
            let empty_msg = Paragraph::new("\n  No records found / Run a SELECT query to pull data.")
                .block(block)
                .style(Style::default().fg(theme.text_secondary));
            f.render_widget(empty_msg, area);
            return;
        }

        let total_cols = state.result_headers.len();
        let offset = state.col_scroll_offset.min(total_cols);
        let visible_headers = &state.result_headers[offset..];
        
        let max_visible_cols = (area.width / 18) as usize;
        let max_visible_cols = max_visible_cols.max(1);
        let visible_count = visible_headers.len().min(max_visible_cols);
        let sliced_headers = &visible_headers[0..visible_count];
        
        let widths = vec![Constraint::Percentage(100 / visible_count.max(1) as u16); visible_count];

        let header_cells = sliced_headers
            .iter()
            .map(|h| ratatui::widgets::Cell::from(h.as_str()).style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)));
        let header = Row::new(header_cells).height(1).bottom_margin(1);

        let rows: Vec<Row> = state
            .result_rows
            .iter()
            .enumerate()
            .map(|(idx, row_cells)| {
                let offset_cells = &row_cells[offset.min(row_cells.len())..];
                let visible_cells = &offset_cells[0..visible_count.min(offset_cells.len())];
                let cells = visible_cells.iter().map(|c| ratatui::widgets::Cell::from(c.as_str()).style(Style::default().fg(theme.text_primary)));
                let is_selected = state.selected_row_idx == Some(idx);
                let row = Row::new(cells).height(1);
                if is_selected {
                    row.style(Style::default().bg(Color::Rgb(49, 50, 68)).fg(theme.border_active).add_modifier(Modifier::BOLD))
                } else {
                    row
                }
            })
            .collect();

        let scroll_indicator = if total_cols > visible_count {
            format!(" (Col {}/{} ◀/▶ to scroll)", offset + 1, total_cols)
        } else {
            "".to_string()
        };
        let focused_grid = state.active_pane == ActivePane::QueryResults;
        let final_block = get_pane_block(&format!("DATA VIEW{}", scroll_indicator), focused_grid, theme);

        let table = Table::new(rows, widths)
            .header(header)
            .block(final_block)
            .highlight_style(Style::default().bg(Color::Rgb(49, 50, 68)))
            .highlight_symbol(">> ");

        let mut table_state = ratatui::widgets::TableState::default();
        table_state.select(state.selected_row_idx);

        f.render_stateful_widget(table, area, &mut table_state);
    }
}

fn draw_related_data_split(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    if state.relationships.is_empty() {
        state.rect_related_list = None;
        state.rect_related_grid = None;
        return;
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30), // List of joins/relationships
            Constraint::Percentage(70), // Table of matching rows
        ])
        .split(area);

    state.rect_related_list = Some(columns[0]);
    state.rect_related_grid = Some(columns[1]);

    // 1. Draw Left Pane: List of relations
    let is_list_focused = state.active_pane == ActivePane::RelatedDataList;
    let list_block = get_pane_block(" Joins List (Up/Down) ", is_list_focused, theme);
    
    let items: Vec<ListItem> = state
        .relationships
        .iter()
        .enumerate()
        .map(|(idx, rel)| {
            let rel_type = if rel.is_parent { "parent" } else { "child" };
            let text = format!("{} ({}: {})", rel.target_table, rel_type, rel.active_col);
            let style = if state.active_relationship_idx == idx {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_primary)
            };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(list_block)
        .highlight_style(Style::default().bg(Color::Rgb(49, 50, 68)))
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    list_state.select(Some(state.active_relationship_idx));
    f.render_stateful_widget(list, columns[0], &mut list_state);

    // 2. Draw Right Pane: Matching rows table
    let is_grid_focused = state.active_pane == ActivePane::RelatedDataGrid;
    
    if state.related_rows.is_empty() {
        let grid_block = get_pane_block(" Matching Rows ", is_grid_focused, theme);
        let no_data_msg = if state.related_loading {
            "Loading referenced records... Please wait."
        } else {
            "No matching related records found."
        };
        let empty_para = Paragraph::new(format!("\n  {}", no_data_msg))
            .block(grid_block)
            .style(Style::default().fg(theme.text_secondary));
        f.render_widget(empty_para, columns[1]);
        return;
    }

    let total_cols = state.related_headers.len();
    let offset = state.related_col_scroll_offset.min(total_cols);
    let visible_headers = &state.related_headers[offset..];

    let max_visible_cols = (columns[1].width / 18) as usize;
    let max_visible_cols = max_visible_cols.max(1);
    let visible_count = visible_headers.len().min(max_visible_cols);
    let sliced_headers = &visible_headers[0..visible_count];

    let widths = vec![Constraint::Percentage(100 / visible_count.max(1) as u16); visible_count];

    let header_cells = sliced_headers
        .iter()
        .map(|h| ratatui::widgets::Cell::from(h.as_str()).style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows: Vec<Row> = state
        .related_rows
        .iter()
        .enumerate()
        .map(|(idx, row_cells)| {
            let offset_cells = &row_cells[offset.min(row_cells.len())..];
            let visible_cells = &offset_cells[0..visible_count.min(offset_cells.len())];
            let cells = visible_cells.iter().map(|c| ratatui::widgets::Cell::from(c.as_str()).style(Style::default().fg(theme.text_primary)));
            let is_selected = state.related_selected_row_idx == Some(idx);
            let row = Row::new(cells).height(1);
            if is_selected {
                row.style(Style::default().bg(Color::Rgb(49, 50, 68)).fg(theme.border_active).add_modifier(Modifier::BOLD))
            } else {
                row
            }
        })
        .collect();

    let scroll_indicator = if total_cols > visible_count {
        format!(" (Col {}/{} ◀/▶)", offset + 1, total_cols)
    } else {
        "".to_string()
    };

    let grid_block = get_pane_block(
        &format!(" Matching Rows{} (Up/Down to scroll, Tab to switch) ", scroll_indicator),
        is_grid_focused,
        theme
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(grid_block)
        .highlight_style(Style::default().bg(Color::Rgb(49, 50, 68)))
        .highlight_symbol("> ");

    let mut table_state = ratatui::widgets::TableState::default();
    table_state.select(state.related_selected_row_idx);

    f.render_stateful_widget(table, columns[1], &mut table_state);
}

fn draw_bi_dashboard(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let is_focused = state.active_pane == ActivePane::QueryResults;
    let outer_block = get_pane_block(" BI DYNAMIC PIVOT PANEL (1/2/3 to switch tabs) ", is_focused, theme);
    f.render_widget(outer_block.clone(), area);
    
    let inner_area = outer_block.inner(area);
    
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35), // Left: Configurator
            Constraint::Percentage(65), // Right: Pivot Table & Charts
        ])
        .split(inner_area);
        
    state.rect_bi_config = Some(chunks[0]);
    state.rect_bi_pivot = Some(chunks[1]);

    // 1. Left Configurator Block
    let config_block = Block::bordered()
        .title(" CONFIGURATOR (Up/Down to select, Left/Right to change) ")
        .border_style(Style::default().fg(theme.border_inactive));
    let config_area = config_block.inner(chunks[0]);
    f.render_widget(config_block, chunks[0]);
    
    let num_cols = if state.pivot_state.config.bi_source_related {
        state.related_headers.len()
    } else {
        state.result_headers.len()
    };
    
    // Helper to get column name
    let get_col_name = |idx_opt: Option<usize>| -> String {
        match idx_opt {
            Some(idx) => {
                if idx < num_cols {
                    if state.pivot_state.config.bi_source_related {
                        state.related_headers[idx].clone()
                    } else {
                        state.result_headers[idx].clone()
                    }
                } else {
                    "[ INVALID ]".to_string()
                }
            }
            None => "[ NONE / Count All ]".to_string(),
        }
    };

    let get_filter_col_name = |idx_opt: Option<usize>| -> String {
        match idx_opt {
            Some(idx) => {
                if idx < num_cols {
                    if state.pivot_state.config.bi_source_related {
                        state.related_headers[idx].clone()
                    } else {
                        state.result_headers[idx].clone()
                    }
                } else {
                    "[ INVALID ]".to_string()
                }
            }
            None => "[ NO FILTER ]".to_string(),
        }
    };
    
    let active_idx = state.pivot_state.active_selector_idx;
    
    let rows_str = get_col_name(state.pivot_state.config.row_dimension_idx);
    let cols_str = get_col_name(state.pivot_state.config.col_dimension_idx);
    let vals_str = get_col_name(state.pivot_state.config.value_column_idx);
    let agg_str = state.pivot_state.config.agg_fn.label().to_string();
    let filter_col_str = get_filter_col_name(state.pivot_state.config.filter_col_idx);
    let filter_op_str = state.pivot_state.config.filter_op.clone();
    let filter_val_str = if active_idx == 6 {
        format!("{}█", state.pivot_state.filter_text_input) // cursor indicator
    } else {
        state.pivot_state.config.filter_val.clone()
    };
    let chart_str = state.pivot_state.config.chart_type.label().to_string();
    let auto_recalc_str = if state.pivot_state.config.auto_recalc {
        "[X] Automatic (Real-time)"
    } else {
        "[ ] Manual (Ctrl+X to run)"
    }.to_string();
    let rate_base_str = get_col_name(state.pivot_state.config.rate_base_column_idx);
    let bi_source_str = if state.pivot_state.config.bi_source_related {
        "Related Table (Parent/Child Split)"
    } else {
        "Main Table (Grid Query Results)"
    }.to_string();
    
    let items = vec![
        ("Rows (Dimension)", rows_str),
        ("Columns (Dimension)", cols_str),
        ("Values (Measure)", vals_str),
        ("Aggr. Function", agg_str),
        ("Filter Column", filter_col_str),
        ("Filter Operator", filter_op_str),
        ("Filter Value", filter_val_str),
        ("Chart Representation", chart_str),
        ("Recalculate Mode", auto_recalc_str),
        ("Rate Base Column (Denom.)", rate_base_str),
        ("BI Data Source", bi_source_str),
    ];
    
    let config_splits = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(22), // 11 items * 2 lines each
            Constraint::Min(5),    // Verbose Statistics Summary box
        ])
        .split(config_area);

    let config_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(config_splits[0]);
        
    for (i, (label, value)) in items.iter().enumerate() {
        let is_active = active_idx == i;
        let style = if is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_secondary)
        };
        
        let text = Line::from(vec![
            Span::styled(format!(" {} : ", label), style),
            Span::styled(value, Style::default().fg(theme.text_primary)),
        ]);
        
        let border_style = if is_active {
            Style::default().fg(theme.border_active)
        } else {
            Style::default().fg(theme.border_inactive)
        };
        let row_block = Block::default().borders(Borders::BOTTOM).border_style(border_style);
        f.render_widget(Paragraph::new(text).block(row_block), config_layout[i]);
    }

    // Statistics Summary Box
    let raw_rows_count = if state.pivot_state.config.bi_source_related {
        state.related_rows.len()
    } else {
        state.result_rows.len()
    };
    let filtered_rows_count = state.pivot_state.pivot_rows.len().saturating_sub(1);
    let cols_count = state.pivot_state.pivot_headers.len().saturating_sub(2);
    let grand_total = state.pivot_state.pivot_rows.last().and_then(|row| row.last()).cloned().unwrap_or_else(|| "N/A".to_string());

    let stats_block = Block::bordered()
        .title(" BI Statistics Summary ")
        .border_style(Style::default().fg(theme.border_inactive));
    
    let stats_text = vec![
        Line::from(vec![
            Span::styled(" Raw Data Rows: ", Style::default().fg(theme.text_secondary)),
            Span::styled(format!("{}", raw_rows_count), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(" Rows after Filter: ", Style::default().fg(theme.text_secondary)),
            Span::styled(format!("{}", filtered_rows_count), Style::default().fg(theme.success)),
        ]),
        Line::from(vec![
            Span::styled(" Column Categories: ", Style::default().fg(theme.text_secondary)),
            Span::styled(format!("{}", cols_count), Style::default().fg(theme.accent)),
        ]),
        Line::from(vec![
            Span::styled(" Grand Total Sum/Rate: ", Style::default().fg(theme.text_secondary)),
            Span::styled(grand_total, Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)),
        ]),
    ];
    
    let stats_para = Paragraph::new(stats_text).block(stats_block);
    f.render_widget(stats_para, config_splits[1]);
    
    // 2. Right Pivot Result Panel
    let result_block = Block::bordered()
        .title(" PIVOT MATRIX / GRAPH VIEW ")
        .border_style(Style::default().fg(theme.border_inactive));
    let result_area = result_block.inner(chunks[1]);
    f.render_widget(result_block, chunks[1]);
    
    if state.pivot_state.pivot_headers.is_empty() || state.pivot_state.pivot_rows.is_empty() {
        let empty_msg = Paragraph::new("\n  No data to pivot. Configure rows/columns and values to build dynamic matrix.")
            .style(Style::default().fg(theme.text_secondary));
        f.render_widget(empty_msg, result_area);
        return;
    }
    
    // Depending on chart type, draw Table or Chart
    match state.pivot_state.config.chart_type {
        BiChartType::TableOnly => {
            // Draw Table
            let cols_count = state.pivot_state.pivot_headers.len();
            let widths = vec![Constraint::Percentage(100 / cols_count as u16); cols_count];
            
            let header_cells = state.pivot_state.pivot_headers.iter().map(|h| {
                ratatui::widgets::Cell::from(h.as_str()).style(Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD))
            });
            let header = Row::new(header_cells).height(1).bottom_margin(1);
            
            let rows: Vec<Row> = state.pivot_state.pivot_rows.iter().enumerate().map(|(r_idx, row_cells)| {
                let is_grand_total_row = r_idx == state.pivot_state.pivot_rows.len() - 1;
                
                let cells = row_cells.iter().enumerate().map(|(c_idx, c)| {
                    let is_grand_total_col = c_idx == row_cells.len() - 1;
                    
                    let style = if is_grand_total_row || is_grand_total_col {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text_primary)
                    };
                    ratatui::widgets::Cell::from(c.as_str()).style(style)
                });
                
                let row_style = if is_grand_total_row {
                    Style::default().bg(Color::Rgb(49, 50, 68))
                } else {
                    Style::default()
                };
                
                Row::new(cells).height(1).style(row_style)
            }).collect();
            
            let table = Table::new(rows, widths)
                .header(header)
                .block(Block::default());
            f.render_widget(table, result_area);
        }
        BiChartType::Bar => {
            // Draw Bar Chart of pivot row totals
            let bars: Vec<Bar> = state.pivot_state.pivot_chart_data.iter().map(|b| {
                Bar::default()
                    .value(b.value)
                    .label(b.label.as_str().into())
                    .style(Style::default().fg(theme.accent))
                    .value_style(Style::default().fg(Color::Black).bg(theme.accent).add_modifier(Modifier::BOLD))
            }).collect();
            
            let group = BarGroup::default().bars(&bars);
            let barchart = BarChart::default()
                .block(Block::default())
                .data(group)
                .bar_width(12)
                .bar_gap(2)
                .value_style(Style::default().fg(Color::Yellow))
                .label_style(Style::default().fg(theme.text_secondary));
            f.render_widget(barchart, result_area);
        }
        BiChartType::Sparkline => {
            // Draw Sparkline of row values
            let values: Vec<u64> = state.pivot_state.pivot_chart_data.iter().map(|b| b.value).collect();
            let sparkline = Sparkline::default()
                .block(Block::default())
                .data(&values)
                .style(Style::default().fg(theme.success));
            f.render_widget(sparkline, result_area);
        }
    }
}

fn draw_sql_console(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    state.rect_sql_console = Some(area);

    let is_focused = state.active_pane == ActivePane::SqlConsole;
    let console_title = match state.active_engine {
        ActiveEngine::MongoDb => " MONGO FILTER (collection|filter) ",
        _ => " SQL QUERY CONSOLE (Press Enter to execute) ",
    };
    let block = get_pane_block(console_title, is_focused, theme);

    let input_para = Paragraph::new(state.sql_console_input.as_str())
        .block(block)
        .style(Style::default().fg(theme.text_primary));
    
    f.render_widget(input_para, area);

    if is_focused {
        let cursor_x = area.x + state.sql_cursor_pos as u16 + 1;
        let cursor_y = area.y + 1;
        f.set_cursor(cursor_x, cursor_y);
    }
}

fn draw_footer_status(f: &mut Frame, area: Rect, state: &mut AppState, theme: &Theme) {
    let bar_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Connection status/Breadcrumbs (Left)
            Constraint::Percentage(50), // Keyboard shortcuts (Right)
        ])
        .split(area);

    // Left Connection/History status
    let status_style = if state.connecting {
        Style::default().fg(theme.header_fg)
    } else {
        Style::default().fg(theme.success)
    };

    let active_db_indicator = if state.exploration_history.is_empty() {
        format!(" ● Connection: {}", state.conn_status_msg)
    } else {
        let mut path = Vec::new();
        for hist in &state.exploration_history {
            if let Some(ref name) = hist.table_name {
                path.push(name.clone());
            }
        }
        if let Some(ref name) = state.active_table_name {
            path.push(name.clone());
        }
        format!(" ➔ {}", path.join(" ➔ "))
    };
    let status_widget = Paragraph::new(active_db_indicator).style(status_style);
    f.render_widget(status_widget, bar_layout[0]);

    // Right keyboard hotkeys legend
    let bi_avail = if state.bi_chartable { " | F6: BI Chart" } else { "" };
    let mut shortcut_text = format!("Ctrl+Q: Quit | Tab: Switch Pane | Esc: Selector | d: DBs | Enter: Run{}", bi_avail);
    if state.active_pane == ActivePane::RelatedDataGrid {
        shortcut_text = format!("Enter/G: Descend | Backspace: Back | {}", shortcut_text);
    } else if !state.exploration_history.is_empty() {
        shortcut_text = format!("Backspace: Back | {}", shortcut_text);
    }
    let legend = Paragraph::new(shortcut_text)
        .alignment(ratatui::layout::Alignment::Right)
        .style(Style::default().fg(theme.text_secondary));
    f.render_widget(legend, bar_layout[1]);
}

fn draw_row_editor_modal(f: &mut Frame, container_area: Rect, state: &mut AppState, theme: &Theme) {
    let modal_area = get_centered_rect(70, 70, container_area);
    f.render_widget(Clear, modal_area);

    let title = if state.show_edit_modal {
        " [ EDIT ROW ] "
    } else {
        " [ INSERT ROW ] "
    };

    let block = Block::bordered()
        .title(title)
        .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme.border_active))
        .bg(Color::Black);
    f.render_widget(block.clone(), modal_area);

    let inner_area = block.inner(modal_area);

    let len = state.modal_fields.len();
    if len == 0 {
        return;
    }

    let max_visible_fields = 8;
    let active_idx = state.active_modal_field_idx;
    let start_idx = if active_idx >= max_visible_fields {
        active_idx - max_visible_fields + 1
    } else {
        0
    };
    let end_idx = (start_idx + max_visible_fields).min(len);

    let visible_len = end_idx - start_idx;
    let constraints = vec![Constraint::Length(2); visible_len];
    let fields_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner_area);

    for idx in start_idx..end_idx {
        let field_layout_idx = idx - start_idx;
        let (col_name, col_val) = &state.modal_fields[idx];
        let is_active = active_idx == idx;

        let label_style = if is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text_secondary)
        };

        let label_text = format!(" {} : ", col_name);
        let input_line = Line::from(vec![
            Span::styled(label_text, label_style),
            Span::styled(col_val, Style::default().fg(theme.text_primary)),
        ]);

        let block_style = if is_active {
            Style::default().fg(theme.border_active)
        } else {
            Style::default().fg(theme.border_inactive)
        };
        let row_block = Block::default().borders(Borders::BOTTOM).border_style(block_style);
        f.render_widget(Paragraph::new(input_line).block(row_block), fields_layout[field_layout_idx]);

        if is_active && state.active_pane == ActivePane::ModalEditor {
            let label_len = col_name.len() as u16 + 4;
            let cursor_x = fields_layout[field_layout_idx].x + label_len + col_val.len() as u16;
            let cursor_y = fields_layout[field_layout_idx].y;
            f.set_cursor(cursor_x, cursor_y);
        }
    }
}

fn draw_delete_confirm_modal(f: &mut Frame, container_area: Rect, state: &mut AppState, theme: &Theme) {
    let modal_area = get_centered_rect(50, 25, container_area);
    f.render_widget(Clear, modal_area);

    let block = Block::bordered()
        .title(" [ CONFIRM DELETE ] ")
        .title_style(Style::default().fg(theme.danger).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme.danger))
        .bg(Color::Black);

    let pk_desc = if let Some(ref pk) = state.primary_key {
        let val = state.selected_row_pk_val.as_ref().map(|x| x.as_str()).unwrap_or("???");
        format!("(Row identified by PK {} = '{}')", pk, val)
    } else {
        "(Warning: No Primary Key detected! Delete may fail or affect multiple rows.)".to_string()
    };

    let prompt_text = format!(
        "\n  Are you sure you want to delete this row?\n  {}\n\n  Press [Enter] to Delete, [Esc] to Cancel.",
        pk_desc
    );

    let paragraph = Paragraph::new(prompt_text)
        .block(block)
        .style(Style::default().fg(theme.text_primary))
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, modal_area);
}
