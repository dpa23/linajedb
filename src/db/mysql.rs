use sqlx::{mysql::MySqlRow, Column, MySqlPool, Row, TypeInfo, ValueRef};
use std::time::Duration;

pub async fn connect_mysql(url: &str) -> Result<MySqlPool, sqlx::Error> {
    sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .connect(url)
        .await
}

pub async fn list_mysql_tables(pool: &MySqlPool) -> Result<Vec<String>, sqlx::Error> {
    // MySQL query to fetch user tables
    let rows = sqlx::query("SHOW TABLES")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.get::<String, _>(0)).collect())
}

pub async fn list_mysql_databases(pool: &MySqlPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query("SHOW DATABASES")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().map(|r| r.get::<String, _>(0)).collect())
}

pub fn format_mysql_cell(row: &MySqlRow, index: usize) -> String {
    if row.try_get_raw(index).map(|raw| raw.is_null()).unwrap_or(true) {
        return "NULL".to_string();
    }
    let column = &row.columns()[index];
    let type_name = column.type_info().name().to_uppercase();
    let type_name_str = type_name.as_str();

    if type_name_str.contains("CHAR") || type_name_str.contains("TEXT") || type_name_str == "JSON" || type_name_str == "ENUM" || type_name_str == "SET" {
        row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
    } else if type_name_str.contains("INT") || type_name_str == "INTEGER" || type_name_str == "BIT" {
        let is_unsigned = type_name_str.contains("UNSIGNED");
        if is_unsigned {
            if let Ok(val) = row.try_get::<u64, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<u32, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<u16, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<u8, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<i64, _>(index) {
                val.to_string()
            } else {
                row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
            }
        } else {
            if let Ok(val) = row.try_get::<i64, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<i32, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<i16, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<i8, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<u64, _>(index) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<bool, _>(index) {
                val.to_string()
            } else {
                row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
            }
        }
    } else if type_name_str == "BOOLEAN" || type_name_str == "BOOL" {
        if let Ok(val) = row.try_get::<bool, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<i8, _>(index) {
            (val != 0).to_string()
        } else if let Ok(val) = row.try_get::<u8, _>(index) {
            (val != 0).to_string()
        } else {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
        }
    } else if type_name_str.contains("FLOAT") || type_name_str.contains("DOUBLE") || type_name_str.contains("DECIMAL") || type_name_str.contains("NUMERIC") {
        if let Ok(val) = row.try_get::<f64, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<sqlx::types::BigDecimal, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<f32, _>(index) {
            val.to_string()
        } else {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
        }
    } else if type_name_str.contains("DATE") || type_name_str.contains("TIME") || type_name_str == "TIMESTAMP" || type_name_str == "YEAR" {
        if let Ok(val) = row.try_get::<chrono::NaiveDateTime, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<chrono::NaiveDate, _>(index) {
            val.to_string()
        } else {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "[DateTime]".into())
        }
    } else {
        if let Ok(val) = row.try_get::<String, _>(index) {
            val
        } else if let Ok(val) = row.try_get::<Vec<u8>, _>(index) {
            format!("[BLOB ({} bytes)]", val.len())
        } else if let Ok(val) = row.try_get::<i64, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<u64, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<bool, _>(index) {
            val.to_string()
        } else {
            format!("[Type: {}]", type_name)
        }
    }
}

pub async fn execute_mysql_query(
    pool: &MySqlPool,
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
            row.push(format_mysql_cell(&sqlx_row, i));
        }
        rows.push(row);
    }

    Ok((columns, rows))
}

pub async fn fetch_mysql_metadata(
    pool: &MySqlPool,
    table: &str,
) -> Result<(Option<String>, Vec<crate::db::RelationshipInfo>), sqlx::Error> {
    // 1. Fetch Primary Key
    let pk_query = "
        SELECT COLUMN_NAME 
        FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE 
        WHERE CONSTRAINT_NAME = 'PRIMARY' 
          AND TABLE_SCHEMA = DATABASE() 
          AND TABLE_NAME = ?;
    ";
    let pk_row = sqlx::query_scalar::<_, String>(pk_query)
        .bind(table)
        .fetch_optional(pool)
        .await?;

    let mut relationships = Vec::new();

    // 2. Fetch parents
    let parent_query = "
        SELECT 
            REFERENCED_TABLE_NAME, COLUMN_NAME, REFERENCED_COLUMN_NAME
        FROM 
            INFORMATION_SCHEMA.KEY_COLUMN_USAGE
        WHERE 
            TABLE_SCHEMA = DATABASE() 
            AND TABLE_NAME = ?
            AND REFERENCED_TABLE_NAME IS NOT NULL;
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
            TABLE_NAME, REFERENCED_COLUMN_NAME, COLUMN_NAME
        FROM 
            INFORMATION_SCHEMA.KEY_COLUMN_USAGE
        WHERE 
            TABLE_SCHEMA = DATABASE() 
            AND REFERENCED_TABLE_NAME = ?
            AND TABLE_NAME != REFERENCED_TABLE_NAME;
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
