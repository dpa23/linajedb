use sqlx::{mysql::{MySqlPoolOptions, MySqlRow}, Row, Column, TypeInfo, ValueRef};
use std::time::Duration;

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
        if let Ok(val) = row.try_get::<i64, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<u64, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<i32, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<u32, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<i16, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<u16, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<i8, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<u8, _>(index) {
            val.to_string()
        } else if let Ok(val) = row.try_get::<bool, _>(index) {
            val.to_string()
        } else {
            row.try_get::<String, _>(index).unwrap_or_else(|_| "???".into())
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "mysql://root:1234@127.0.0.1:3306/INOPCONBD";
    let pool = MySqlPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(url)
        .await?;

    let sql = "SELECT * FROM permisos LIMIT 5";
    let rows = sqlx::query(sql)
        .fetch_all(&pool)
        .await?;

    println!("Query result for: {}", sql);
    if rows.is_empty() {
        println!("No rows returned!");
        return Ok(());
    }

    let cols = rows[0].columns();
    println!("Columns:");
    for (i, col) in cols.iter().enumerate() {
        println!("  Column #{}: '{}' (Type: {})", i, col.name(), col.type_info().name());
    }

    println!("Rows:");
    for (row_idx, row) in rows.iter().enumerate() {
        print!("  Row #{}: ", row_idx);
        for i in 0..cols.len() {
            let formatted = format_mysql_cell(row, i);
            print!("{}: '{}'  ", cols[i].name(), formatted);
        }
        println!();
    }

    Ok(())
}
