//! Generic, model-free table rendering for any service in the catalog.
//!
//! Curated views in [`crate::services`] give the best experience for a handful
//! of services. This module is the fallback that makes the other ~420 services
//! browsable: it takes a response, finds the most useful array of records in it,
//! derives columns from the records, and finds the pagination token.

use serde_json::{Map, Value};

use crate::client::{ApiError, ApiResponse, EmulatorClient};

/// One row produced from an arbitrary response.
#[derive(Clone, Debug)]
pub struct GenericRow {
    pub cells: Vec<String>,
    pub detail: Value,
}

/// A table extracted from an arbitrary response.
#[derive(Clone, Debug, Default)]
pub struct GenericTable {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<GenericRow>,
    pub token: Option<Token>,
}

/// A pagination token found in a response.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// Field in the response that carried the token.
    pub field: String,
    /// Field to set on the next request.
    pub input_field: String,
    pub value: Value,
}

impl Token {
    /// Return the input for the next page, based on the input of this page.
    pub fn next_input(&self, previous: &Value) -> Value {
        let mut input = previous.as_object().cloned().unwrap_or_default();
        input.insert(self.input_field.clone(), self.value.clone());
        Value::Object(input)
    }
}

/// The response as JSON, converting XML responses on the fly.
pub fn response_value(response: &ApiResponse) -> Option<Value> {
    response
        .json
        .clone()
        .or_else(|| crate::xml::to_json(&response.body))
}

/// Fetch one operation and turn its response into a table.
pub async fn fetch(
    client: &EmulatorClient,
    service: &str,
    operation: &str,
    input: Value,
) -> Result<(GenericTable, ApiResponse), ApiError> {
    let response = client.call(service, operation, input).await?;
    let table = match response_value(&response) {
        Some(value) => table_from_value(&value, operation),
        None => GenericTable {
            title: operation.to_string(),
            ..GenericTable::default()
        },
    };
    Ok((table, response))
}

/// Extract the most useful table from a response value.
pub fn table_from_value(value: &Value, operation: &str) -> GenericTable {
    let mut candidates = Vec::new();
    collect_arrays(value, &mut Vec::new(), &mut candidates, 0);

    let best = candidates
        .iter()
        .filter(|candidate| !candidate.items.is_empty())
        .max_by_key(|candidate| candidate.score())
        .cloned();

    if let Some(candidate) = best {
        let keys = if candidate.items.iter().all(|item| !item.is_object()) {
            vec![candidate
                .path
                .last()
                .cloned()
                .unwrap_or_else(|| "VALUE".to_string())]
        } else {
            derive_columns(&candidate.items)
        };
        let rows = candidate
            .items
            .iter()
            .map(|item| GenericRow {
                cells: keys.iter().map(|key| cell(item, key)).collect(),
                detail: item.clone(),
            })
            .collect();
        return GenericTable {
            title: operation.to_string(),
            columns: keys.iter().map(|key| humanize(key)).collect(),
            rows,
            token: find_token(value),
        };
    }

    // No populated array. If the response names an empty collection, show its
    // column so the empty state still has a header.
    if let Some(candidate) = candidates.first() {
        let column = candidate
            .path
            .last()
            .map(|name| humanize(name))
            .unwrap_or_else(|| "VALUE".to_string());
        return GenericTable {
            title: operation.to_string(),
            columns: vec![column],
            rows: Vec::new(),
            token: None,
        };
    }

    // A flat object: one row of key/value pairs. Nested objects and arrays are
    // already handled by the array search; only show scalar fields here so an
    // empty Describe* response does not become a one-row table of metadata.
    if let Some(map) = value.as_object() {
        let mut columns = Vec::new();
        let mut cells = Vec::new();
        for (key, value) in map {
            if key == "ResponseMetadata" || key == "requestId" || key == "$metadata" {
                continue;
            }
            if !is_scalar(value) {
                continue;
            }
            columns.push(humanize(key));
            cells.push(render(value));
            if columns.len() >= 10 {
                break;
            }
        }
        if !columns.is_empty() {
            return GenericTable {
                title: operation.to_string(),
                columns,
                rows: vec![GenericRow {
                    cells,
                    detail: value.clone(),
                }],
                token: find_token(value),
            };
        }
    }

    GenericTable {
        title: operation.to_string(),
        ..GenericTable::default()
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    path: Vec<String>,
    items: Vec<Value>,
    object_items: usize,
    field_count: usize,
}

impl Candidate {
    fn score(&self) -> i64 {
        let mut score = (self.object_items as i64) * 12
            + (self.field_count as i64)
            + (self.items.len().min(50) as i64)
            - (self.path.len() as i64) * 2;
        // A list of wrappers such as EC2's reservationSet.item (each holding
        // only an instancesSet) is less useful than the instances inside it.
        let containers = self.items.iter().filter(|item| is_wrapper(item)).count();
        if containers == self.object_items && self.object_items > 0 {
            score -= 30;
        }
        if let Some(name) = self.path.last() {
            let lower = name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "items"
                    | "item"
                    | "member"
                    | "contents"
                    | "functions"
                    | "queues"
                    | "topics"
                    | "buckets"
                    | "tables"
                    | "users"
                    | "roles"
                    | "instances"
                    | "clusters"
                    | "repositories"
                    | "secrets"
                    | "parameters"
                    | "rules"
                    | "executions"
            ) {
                score += 25;
            }
        }
        score
    }
}

fn collect_arrays(value: &Value, path: &mut Vec<String>, out: &mut Vec<Candidate>, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(items) => {
            let object_items = items.iter().filter(|item| item.is_object()).count();
            let field_count = items
                .iter()
                .filter_map(Value::as_object)
                .map(Map::len)
                .sum();
            out.push(Candidate {
                path: path.clone(),
                items: items.clone(),
                object_items,
                field_count,
            });
            if let Some(first) = items.first() {
                collect_arrays(first, path, out, depth + 1);
            }
        }
        Value::Object(map) => {
            for (key, child) in map {
                path.push(key.clone());
                collect_arrays(child, path, out, depth + 1);
                // XML list wrappers hold a single <member>/<item> as a plain
                // object when the list has one entry. Treat it as a list of one.
                if is_list_wrapper(key) && child.is_object() {
                    let fields = child.as_object().map(Map::len).unwrap_or(0);
                    out.push(Candidate {
                        path: path.clone(),
                        items: vec![child.clone()],
                        object_items: 1,
                        field_count: fields,
                    });
                }
                path.pop();
            }
        }
        _ => {}
    }
}

fn is_list_wrapper(key: &str) -> bool {
    matches!(
        key,
        "member"
            | "item"
            | "entry"
            | "Bucket"
            | "Contents"
            | "CommonPrefixes"
            | "Upload"
            | "DeleteMarker"
            | "Alias"
            | "Grant"
            | "Rule"
    )
}

fn is_wrapper(item: &Value) -> bool {
    let Some(map) = item.as_object() else {
        return false;
    };
    map.len() == 1 && map.values().next().is_some_and(|value| !is_scalar(value))
}

fn derive_columns(items: &[Value]) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for item in items.iter().take(20) {
        let Some(map) = item.as_object() else {
            continue;
        };
        for key in map.keys() {
            if key == "ResponseMetadata" {
                continue;
            }
            if !columns.iter().any(|column| column == key) {
                columns.push(key.clone());
            }
        }
    }
    if columns.is_empty() {
        columns.push("VALUE".to_string());
    }
    columns.sort_by_key(|column| column_priority(column));
    columns.truncate(8);
    columns
}

fn column_priority(key: &str) -> u8 {
    let lower = key.to_ascii_lowercase();
    if lower == "id" || lower.ends_with("id") || lower == "key" {
        0
    } else if lower == "name" || lower.ends_with("name") {
        1
    } else if lower.ends_with("arn") {
        2
    } else if lower.ends_with("url") || lower.ends_with("uri") {
        3
    } else {
        4
    }
}

fn cell(item: &Value, key: &str) -> String {
    let Some(map) = item.as_object() else {
        return render(item);
    };
    map.get(key).map(render).unwrap_or_default()
}

/// Render one JSON value for a table cell.
pub fn render(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Object(map) => {
            if let Some(inner) = attribute_value(map) {
                return inner;
            }
            if map.len() <= 4 && map.values().all(is_scalar) {
                return map
                    .iter()
                    .map(|(key, value)| format!("{key}={}", scalar(value)))
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            truncate(&serde_json::to_string(value).unwrap_or_default(), 80)
        }
        Value::Array(items) => {
            if items.iter().all(is_scalar) {
                items.iter().map(scalar).collect::<Vec<_>>().join(", ")
            } else {
                truncate(&serde_json::to_string(value).unwrap_or_default(), 80)
            }
        }
    }
}

fn attribute_value(map: &Map<String, Value>) -> Option<String> {
    if map.len() != 1 {
        return None;
    }
    let (kind, value) = map.iter().next()?;
    let rendered = match kind.as_str() {
        "S" | "N" | "B" | "BOOL" | "NULL" => scalar(value),
        "SS" | "NS" | "BS" | "L" => match value {
            Value::Array(items) => items.iter().map(render).collect::<Vec<_>>().join(", "),
            other => render(other),
        },
        "M" => match value {
            Value::Object(_) => truncate(&serde_json::to_string(value).unwrap_or_default(), 80),
            other => render(other),
        },
        _ => return None,
    };
    Some(rendered)
}

fn is_scalar(value: &Value) -> bool {
    !value.is_object() && !value.is_array()
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(limit.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn humanize(key: &str) -> String {
    let mut out = String::new();
    let mut previous_lower = false;
    for character in key.chars() {
        if character == '_' {
            out.push(' ');
            previous_lower = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_lower {
            out.push(' ');
        }
        out.push(character.to_ascii_uppercase());
        previous_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
    }
    out
}

/// Find the pagination token in a response, if any.
pub fn find_token(value: &Value) -> Option<Token> {
    let mut tokens = Vec::new();
    collect_tokens(value, 0, &mut tokens);
    tokens
        .into_iter()
        .min_by_key(|(depth, _)| *depth)
        .map(|(_, token)| token)
}

fn collect_tokens(value: &Value, depth: usize, out: &mut Vec<(usize, Token)>) {
    if depth > 6 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let input_field = match key.as_str() {
                    "NextToken" | "nextToken" => Some("NextToken"),
                    "NextMarker" | "Marker" | "nextMarker" => Some("Marker"),
                    "NextContinuationToken" | "continuationToken" => Some("ContinuationToken"),
                    "nextPageToken" | "pageToken" => Some("nextPageToken"),
                    "LastEvaluatedKey" => Some("ExclusiveStartKey"),
                    "NextKeyMarker" => Some("KeyMarker"),
                    "NextUploadIdMarker" => Some("UploadIdMarker"),
                    _ => None,
                };
                if let Some(input_field) = input_field {
                    if !is_empty(child) {
                        out.push((
                            depth,
                            Token {
                                field: key.clone(),
                                input_field: input_field.to_string(),
                                value: child.clone(),
                            },
                        ));
                    }
                }
                collect_tokens(child, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(20) {
                collect_tokens(item, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_dynamodb_items_and_unwraps_attributes() {
        let value = json!({
            "Items": [
                {"id": {"S": "a"}, "total": {"N": "10"}, "tags": {"SS": ["x", "y"]}},
                {"id": {"S": "b"}, "total": {"N": "20"}}
            ],
            "Count": 2,
            "LastEvaluatedKey": {"id": {"S": "b"}}
        });
        let table = table_from_value(&value, "Scan");
        assert_eq!(table.columns, vec!["ID", "TOTAL", "TAGS"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells[0], "a");
        assert_eq!(table.rows[0].cells[1], "10");
        assert_eq!(table.rows[0].cells[2], "x, y");
        let token = table.token.expect("token");
        assert_eq!(token.input_field, "ExclusiveStartKey");
    }

    #[test]
    fn extracts_nested_ec2_instances() {
        let value = json!({
            "reservationSet": {
                "item": [{
                    "instancesSet": {
                        "item": [{
                            "instanceId": "i-1",
                            "instanceType": "t3.micro",
                            "instanceState": {"name": "running", "code": 16},
                            "privateIpAddress": "10.0.0.1"
                        }]
                    }
                }]
            }
        });
        let table = table_from_value(&value, "DescribeInstances");
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells[0], "i-1");
        assert_eq!(table.columns[0], "INSTANCE ID");
        assert!(table.rows[0].cells[2].contains("name=running"));
    }

    #[test]
    fn extracts_scalar_lists() {
        let value = json!({"QueueUrls": ["http://q/1", "http://q/2"]});
        let table = table_from_value(&value, "ListQueues");
        assert_eq!(table.columns, vec!["QUEUE URLS"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells[0], "http://q/1");
    }

    #[test]
    fn keeps_empty_collections() {
        let value = json!({"Functions": []});
        let table = table_from_value(&value, "ListFunctions");
        assert!(table.rows.is_empty());
        assert_eq!(table.columns, vec!["FUNCTIONS"]);
    }

    #[test]
    fn finds_tokens() {
        let value = json!({"Result": {"NextToken": "abc"}});
        let token = find_token(&value).unwrap();
        assert_eq!(token.field, "NextToken");
        assert_eq!(token.input_field, "NextToken");
        assert_eq!(token.value, json!("abc"));
        assert_eq!(
            token.next_input(&json!({"TableName": "t"})),
            json!({"TableName": "t", "NextToken": "abc"})
        );
    }

    #[test]
    fn humanizes_names() {
        assert_eq!(humanize("FunctionName"), "FUNCTION NAME");
        assert_eq!(humanize("instanceId"), "INSTANCE ID");
        assert_eq!(humanize("QueueUrl"), "QUEUE URL");
        assert_eq!(humanize("snake_case"), "SNAKE CASE");
    }
}
