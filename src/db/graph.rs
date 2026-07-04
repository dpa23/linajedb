use neo4rs::{Graph, Node, Relation};

pub async fn connect_neo4j(url: &str, user: &str, pass: &str) -> Result<Graph, neo4rs::Error> {
    Graph::new(url, user, pass).await
}

pub async fn list_neo4j_labels(graph: &Graph) -> Result<Vec<String>, neo4rs::Error> {
    let mut result = graph.execute(neo4rs::query("CALL db.labels()")).await?;
    let mut labels = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        if let Ok(label) = row.get::<String>("label") {
            labels.push(label);
        }
    }
    Ok(labels)
}

pub fn parse_cypher_aliases(query: &str) -> Vec<String> {
    let query_upper = query.to_uppercase();
    if let Some(return_idx) = query_upper.rfind("RETURN ") {
        let after_return = &query[return_idx + 7..];
        let end_idx = after_return.to_uppercase().find(" LIMIT")
            .or_else(|| after_return.to_uppercase().find(" ORDER BY"))
            .or_else(|| after_return.to_uppercase().find(" SKIP"))
            .unwrap_or(after_return.len());
        
        let select_expr = &after_return[..end_idx];
        return select_expr
            .split(',')
            .map(|s| {
                let s = s.trim();
                if let Some(as_idx) = s.to_uppercase().find(" AS ") {
                    s[as_idx + 4..].trim().to_string()
                } else {
                    s.trim().to_string()
                }
            })
            .filter(|s| !s.is_empty())
            .collect();
    }
    vec!["result".to_string()]
}

pub fn format_neo4j_cell(row: &neo4rs::Row, alias: &str) -> String {
    // 1. Try extracting as Node
    if let Ok(node) = row.get::<Node>(alias) {
        let labels_str = node.labels().join(":");
        return format!("Node(id={}, labels=[{}])", node.id(), labels_str);
    }
    // 2. Try extracting as Relation
    if let Ok(rel) = row.get::<Relation>(alias) {
        return format!("Rel(id={}, type={}, {}->{})", rel.id(), rel.typ(), rel.start_node_id(), rel.end_node_id());
    }
    // 3. Try extracting as String
    if let Ok(val) = row.get::<String>(alias) {
        return val;
    }
    // 4. Try extracting as i64
    if let Ok(val) = row.get::<i64>(alias) {
        return val.to_string();
    }
    // 5. Try extracting as f64
    if let Ok(val) = row.get::<f64>(alias) {
        return val.to_string();
    }
    // 6. Try extracting as bool
    if let Ok(val) = row.get::<bool>(alias) {
        return val.to_string();
    }

    "[Graph Element]".to_string()
}

pub async fn execute_neo4j_query(
    graph: &Graph,
    cypher: &str,
) -> Result<(Vec<String>, Vec<Vec<String>>), neo4rs::Error> {
    let mut result = graph.execute(neo4rs::query(cypher)).await?;
    let aliases = parse_cypher_aliases(cypher);

    let mut rows = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let mut row_cells = Vec::new();
        for alias in &aliases {
            row_cells.push(format_neo4j_cell(&row, alias));
        }
        rows.push(row_cells);
    }

    if rows.is_empty() {
        return Ok((
            vec!["Status".to_string()],
            vec![vec!["Query returned 0 rows / Command executed successfully".to_string()]],
        ));
    }

    Ok((aliases, rows))
}
