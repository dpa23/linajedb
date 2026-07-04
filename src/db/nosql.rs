use futures::stream::TryStreamExt;
use mongodb::{options::ClientOptions, Client};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;
use serde_json::Value;

pub async fn connect_mongodb(uri: &str) -> Result<Client, mongodb::error::Error> {
    let mut client_options = ClientOptions::parse(uri).await?;
    client_options.connect_timeout = Some(Duration::from_secs(3));
    Client::with_options(client_options)
}

pub async fn list_mongodb_databases(client: &Client) -> Result<Vec<String>, mongodb::error::Error> {
    client.list_database_names(None, None).await
}

pub async fn list_mongodb_collections(
    client: &Client,
    database_name: &str,
) -> Result<Vec<String>, mongodb::error::Error> {
    let db = client.database(database_name);
    db.list_collection_names(None).await
}

pub fn parse_json_filter(filter_str: &str) -> Result<bson::Document, String> {
    if filter_str.trim().is_empty() {
        return Ok(bson::Document::new());
    }
    let json_val: Value = serde_json::from_str(filter_str)
        .map_err(|e| format!("Invalid JSON filter: {}", e))?;
    match bson::to_bson(&json_val) {
        Ok(bson::Bson::Document(doc)) => Ok(doc),
        _ => Err("Filter must be a valid JSON Object".to_string()),
    }
}

/// Like `execute_mongodb_find` but keeps the raw BSON documents, so callers
/// can reuse typed values (_id and friends) in follow-up queries.
pub async fn find_bson_docs(
    client: &Client,
    database_name: &str,
    collection_name: &str,
    filter: bson::Document,
    limit: i64,
) -> Result<Vec<bson::Document>, String> {
    let db = client.database(database_name);
    let collection = db.collection::<bson::Document>(collection_name);
    let find_options = mongodb::options::FindOptions::builder().limit(limit).build();
    let mut cursor = collection
        .find(filter, find_options)
        .await
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        results.push(doc);
    }
    Ok(results)
}

pub async fn execute_mongodb_find(
    client: &Client,
    database_name: &str,
    collection_name: &str,
    filter_str: &str,
    limit: i64,
) -> Result<Vec<Value>, String> {
    let db = client.database(database_name);
    let collection = db.collection::<bson::Document>(collection_name);

    // Parse text input query to filter document
    let filter_doc = parse_json_filter(filter_str)?;

    let find_options = mongodb::options::FindOptions::builder()
        .limit(limit)
        .build();

    let mut cursor = collection.find(filter_doc, find_options)
        .await
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    while let Some(doc) = cursor.try_next().await.map_err(|e| e.to_string())? {
        // Convert bson::Document to serde_json::Value
        if let Ok(json_val) = serde_json::to_value(&doc) {
            results.push(json_val);
        }
    }

    Ok(results)
}

// Local File Readers
pub fn read_local_json_file(path_str: &str) -> Result<Value, String> {
    let path = Path::new(path_str);
    let file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).map_err(|e| format!("Failed to parse JSON: {}", e))
}

pub fn read_local_bson_file(path_str: &str) -> Result<Value, String> {
    let path = Path::new(path_str);
    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {}", e))?;
    let doc = bson::Document::from_reader(&mut file).map_err(|e| format!("Failed to parse BSON: {}", e))?;
    serde_json::to_value(&doc).map_err(|e| format!("Failed to serialize BSON to JSON: {}", e))
}
