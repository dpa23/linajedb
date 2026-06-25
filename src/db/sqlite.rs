use sqlx::{sqlite::SqliteRow, sqlite::SqlitePoolOptions, Column, SqlitePool, Row, TypeInfo, ValueRef};
use std::time::Duration;

pub async fn connect_sqlite(path: &str) -> Result<SqlitePool, sqlx::Error> {
    // Setup WAL mode and set a busy timeout
    let connection_string = if path.starts_with("sqlite:") {
        path.to_string()
    } else {
        format!("sqlite://{}", path)
    };

    SqlitePoolOptions::new()
        .max_connections(1) // SQLite is file-based, 1 write connection is recommended
        .acquire_timeout(Duration::from_secs(3))
        .connect(&connection_string)
        .await
}

pub async fn list_sqlite_tables(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    let query = "
        SELECT name 
        FROM sqlite_master 
        WHERE type = 'table' 
          AND name NOT LIKE 'sqlite_%'
        ORDER BY name;
    ";
    let rows = sqlx::query_as::<_, (String,)>(query)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub fn format_sqlite_cell(row: &SqliteRow, index: usize) -> String {
    if row.try_get_raw(index).map(|raw| raw.is_null()).unwrap_or(true) {
        return "NULL".to_string();
    }
    let column = &row.columns()[index];
    let type_name = column.type_info().name();

    match type_name {
        "TEXT" | "VARCHAR" | "CHAR" => {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
        }
        "INTEGER" | "INT" | "BIGINT" => {
            row.try_get::<i64, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "BOOLEAN" | "BOOL" => {
            row.try_get::<bool, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "REAL" | "DOUBLE" | "FLOAT" => {
            row.try_get::<f64, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "BLOB" => {
            if let Ok(bytes) = row.try_get::<Vec<u8>, _>(index) {
                return format!("[BLOB ({} bytes)]", bytes.len());
            }
            "[BLOB]".to_string()
        }
        _ => {
            row.try_get::<String, _>(index).unwrap_or_else(|_| format!("[Type: {}]", type_name))
        }
    }
}

pub async fn execute_sqlite_query(
    pool: &SqlitePool,
    sql: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let sqlx_rows = sqlx::query(sql).fetch_all(&mut *conn).await?;

    if sqlx_rows.is_empty() {
        return Ok((
            vec!["Status".to_string()],
            vec![vec!["Query returned 0 rows / Command executed successfully".to_string()]],
        ));
    }

    let columns: Vec<String> = sqlx_rows[0]
        .columns()
        .iter()
        .map(|col| col.name().to_string())
        .collect();

    let mut rows = Vec::new();
    for sqlx_row in sqlx_rows {
        let mut row = Vec::new();
        for i in 0..sqlx_row.columns().len() {
            row.push(format_sqlite_cell(&sqlx_row, i));
        }
        rows.push(row);
    }

    Ok((columns, rows))
}

pub async fn fetch_sqlite_metadata(
    pool: &SqlitePool,
    table: &str,
) -> Result<(Option<String>, Vec<crate::db::RelationshipInfo>), sqlx::Error> {
    if !table.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(sqlx::Error::Protocol(format!("Invalid table name: {}", table)));
    }

    // 1. Fetch PK
    let pragma_info = format!("PRAGMA table_info({});", table);
    let rows = sqlx::query(&pragma_info).fetch_all(pool).await?;
    let mut pk_row = None;
    for r in rows {
        let is_pk: i64 = r.try_get("pk")?;
        if is_pk > 0 {
            pk_row = Some(r.try_get::<String, _>("name")?);
            break;
        }
    }

    let mut relationships = Vec::new();

    // 2. Fetch parents (tables this table references)
    let pragma_fk = format!("PRAGMA foreign_key_list({});", table);
    let fk_rows = sqlx::query(&pragma_fk).fetch_all(pool).await?;
    for r in fk_rows {
        let target_table: String = r.try_get("table")?;
        let active_col: String = r.try_get("from")?;
        let target_col: String = r.try_get("to")?;
        relationships.push(crate::db::RelationshipInfo {
            is_parent: true,
            target_table,
            active_col,
            target_col,
        });
    }

    // 3. Fetch children (tables that reference this table)
    let list_tables_query = "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%';";
    let all_tables_rows = sqlx::query(list_tables_query).fetch_all(pool).await?;
    for r in all_tables_rows {
        let other_table: String = r.get(0);
        if other_table == table {
            continue;
        }
        let other_pragma = format!("PRAGMA foreign_key_list({});", other_table);
        if let Ok(other_fks) = sqlx::query(&other_pragma).fetch_all(pool).await {
            for fk in other_fks {
                let parent_table: String = fk.try_get("table")?;
                if parent_table == table {
                    let active_col: String = fk.try_get("to")?;
                    let target_col: String = fk.try_get("from")?;
                    relationships.push(crate::db::RelationshipInfo {
                        is_parent: false,
                        target_table: other_table.clone(),
                        active_col,
                        target_col,
                    });
                }
            }
        }
    }

    Ok((pk_row, relationships))
}
