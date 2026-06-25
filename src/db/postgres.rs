use sqlx::{postgres::PgRow, postgres::PgPoolOptions, Column, PgPool, Row, TypeInfo, ValueRef};
use std::time::Duration;

pub async fn connect_postgres(url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(url)
        .await
}

pub async fn list_postgres_tables(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let query = "
        SELECT table_name 
        FROM information_schema.tables 
        WHERE table_schema = 'public' 
          AND table_type = 'BASE TABLE'
        ORDER BY table_name;
    ";
    let rows = sqlx::query_as::<_, (String,)>(query)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub async fn list_postgres_databases(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let query = "
        SELECT datname 
        FROM pg_database 
        WHERE datistemplate = false 
          AND datallowconn = true
        ORDER BY datname;
    ";
    let rows = sqlx::query_as::<_, (String,)>(query)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub fn format_pg_cell(row: &PgRow, index: usize) -> String {
    if row.try_get_raw(index).map(|raw| raw.is_null()).unwrap_or(true) {
        return "NULL".to_string();
    }
    let column = &row.columns()[index];
    let type_name = column.type_info().name();

    match type_name {
        "VARCHAR" | "TEXT" | "BPCHAR" | "NAME" => {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
        }
        "INT2" | "SMALLINT" | "INT4" | "INTEGER" | "INT" => {
            row.try_get::<i32, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "INT8" | "BIGINT" => {
            row.try_get::<i64, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "BOOL" | "BOOLEAN" => {
            row.try_get::<bool, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "FLOAT4" | "REAL" => {
            row.try_get::<f32, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "FLOAT8" | "DOUBLE PRECISION" => {
            row.try_get::<f64, _>(index).map(|v| v.to_string()).unwrap_or_else(|_| "???".into())
        }
        "NUMERIC" | "DECIMAL" => {
            if let Ok(val) = row.try_get::<f64, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<sqlx::types::BigDecimal, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<f32, _>(index) {
                val.to_string()
            } else {
                "???".to_string()
            }
        }
        "UUID" => {
            if let Ok(u) = row.try_get::<uuid::Uuid, _>(index) {
                return u.to_string();
            }
            "UUID".to_string()
        }
        "JSON" | "JSONB" => {
            if let Ok(val) = row.try_get::<serde_json::Value, _>(index) {
                return val.to_string();
            }
            "JSON".to_string()
        }
        "TIMESTAMPTZ" | "TIMESTAMP" => {
            if let Ok(val) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<chrono::NaiveDateTime, _>(index) {
                val.to_string()
            } else {
                row.try_get::<String, _>(index).unwrap_or_else(|_| "[DateTime]".into())
            }
        }
        _ => {
            if type_name.starts_with('_') {
                "[Array]".to_string()
            } else if let Ok(val) = row.try_get::<String, _>(index) {
                val
            } else {
                format!("[Type: {}]", type_name)
            }
        }
    }
}

pub async fn execute_postgres_query(
    pool: &PgPool,
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
            row.push(format_pg_cell(&sqlx_row, i));
        }
        rows.push(row);
    }

    Ok((columns, rows))
}

pub async fn fetch_postgres_metadata(
    pool: &PgPool,
    table: &str,
) -> Result<(Option<String>, Vec<crate::db::RelationshipInfo>), sqlx::Error> {
    // 1. Fetch PK
    let pk_query = "
        SELECT a.attname 
        FROM pg_index i 
        JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey) 
        WHERE i.indrelid = $1::regclass AND i.indisprimary;
    ";
    let pk_row = sqlx::query_scalar::<_, String>(pk_query)
        .bind(table)
        .fetch_optional(pool)
        .await?;

    let mut relationships = Vec::new();

    // 2. Fetch parents
    let parent_query = "
        SELECT
            ccu.table_name AS referenced_table_name,
            kcu.column_name AS column_name,
            ccu.column_name AS referenced_column_name
        FROM
            information_schema.table_constraints AS tc
            JOIN information_schema.key_column_usage AS kcu
              ON tc.constraint_name = kcu.constraint_name
              AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage AS ccu
              ON ccu.constraint_name = tc.constraint_name
        WHERE tc.constraint_type = 'FOREIGN KEY' 
          AND tc.table_name = $1
          AND tc.table_schema = 'public';
    ";
    let parent_rows = sqlx::query_as::<_, (String, String, String)>(parent_query)
        .bind(table)
        .fetch_all(pool)
        .await?;

    for (ref_table, col, ref_col) in parent_rows {
        relationships.push(crate::db::RelationshipInfo {
            is_parent: true,
            target_table: ref_table,
            active_col: col,
            target_col: ref_col,
        });
    }

    // 3. Fetch children
    let child_query = "
        SELECT
            tc.table_name AS child_table_name,
            ccu.column_name AS referenced_column_name,
            kcu.column_name AS column_name
        FROM
            information_schema.table_constraints AS tc
            JOIN information_schema.key_column_usage AS kcu
              ON tc.constraint_name = kcu.constraint_name
              AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage AS ccu
              ON ccu.constraint_name = tc.constraint_name
        WHERE tc.constraint_type = 'FOREIGN KEY' 
          AND ccu.table_name = $1
          AND tc.table_schema = 'public';
    ";
    let child_rows = sqlx::query_as::<_, (String, String, String)>(child_query)
        .bind(table)
        .fetch_all(pool)
        .await?;

    for (t_name, ref_col, col) in child_rows {
        relationships.push(crate::db::RelationshipInfo {
            is_parent: false,
            target_table: t_name,
            active_col: ref_col,
            target_col: col,
        });
    }

    Ok((pk_row, relationships))
}
