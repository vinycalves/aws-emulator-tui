//! Small XML helpers for the query and REST-XML services.
//!
//! Query-protocol services (IAM, SNS, EC2, CloudFormation) and S3 answer XML.
//! The TUI only needs a handful of fields per list call, plus a readable view of
//! a whole document, so this module stays deliberately small.

use roxmltree::{Document, Node};
use serde_json::{Map, Value};

/// Parse an XML document, returning `None` when the body is not XML.
pub fn parse(body: &str) -> Option<Document<'_>> {
    Document::parse(body.trim_start_matches('\u{feff}')).ok()
}

/// Direct children with the given local name (namespace prefixes ignored).
pub fn children<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Vec<Node<'a, 'input>> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == name)
        .collect()
}

/// First direct child with the given local name.
pub fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}

/// Follow a direct-child path from `root`, returning every node at the end.
pub fn path<'a, 'input>(root: Node<'a, 'input>, names: &[&str]) -> Vec<Node<'a, 'input>> {
    let mut current = vec![root];
    for name in names {
        let mut next = Vec::new();
        for node in current {
            next.extend(children(node, name));
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

/// Text content of a node, trimmed.
pub fn text(node: Node<'_, '_>) -> String {
    node.text().unwrap_or_default().trim().to_string()
}

/// Text of the first child with `name`.
pub fn field(node: Node<'_, '_>, name: &str) -> String {
    child(node, name).map(text).unwrap_or_default()
}

/// Attribute value by local name.
pub fn attr<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}

/// Convert an XML document to a JSON value, for display and search.
///
/// Repeated sibling elements become arrays, attributes become `@name` keys, and
/// text-only elements become strings. This is a presentation format, not a
/// faithful XML data model.
pub fn to_json(body: &str) -> Option<Value> {
    let document = parse(body)?;
    let root = document.root_element();
    let mut wrapper = Map::new();
    wrapper.insert(root.tag_name().name().to_string(), node_to_json(root));
    Some(Value::Object(wrapper))
}

pub fn node_to_json(node: Node<'_, '_>) -> Value {
    let element_children: Vec<Node<'_, '_>> = node.children().filter(|n| n.is_element()).collect();
    if element_children.is_empty() {
        let value = text(node);
        if value.is_empty() {
            if node.attributes().len() == 0 {
                return Value::Object(Map::new());
            }
        } else {
            return Value::String(value);
        }
    }

    let mut map = Map::new();
    for attribute in node.attributes() {
        map.insert(
            format!("@{}", attribute.name()),
            Value::String(attribute.value().to_string()),
        );
    }
    for child in element_children {
        let name = child.tag_name().name().to_string();
        let value = node_to_json(child);
        match map.get_mut(&name) {
            Some(Value::Array(items)) => items.push(value),
            Some(existing) => {
                let first = existing.take();
                *existing = Value::Array(vec![first, value]);
            }
            None => {
                map.insert(name, value);
            }
        }
    }
    Value::Object(map)
}

/// Human-readable form of an XML body; falls back to the raw text.
pub fn pretty(body: &str) -> String {
    match to_json(body) {
        Some(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| body.to_string()),
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S3: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner><ID>abc</ID></Owner>
  <Buckets>
    <Bucket><Name>one</Name><CreationDate>2026-01-01T00:00:00Z</CreationDate></Bucket>
    <Bucket><Name>two</Name><CreationDate>2026-01-02T00:00:00Z</CreationDate></Bucket>
  </Buckets>
</ListAllMyBucketsResult>"#;

    #[test]
    fn finds_buckets_by_path() {
        let document = parse(S3).unwrap();
        let buckets = path(document.root_element(), &["Buckets", "Bucket"]);
        assert_eq!(buckets.len(), 2);
        assert_eq!(field(buckets[0], "Name"), "one");
        assert_eq!(field(buckets[1], "Name"), "two");
    }

    #[test]
    fn converts_to_json() {
        let value = to_json(S3).unwrap();
        let buckets = &value["ListAllMyBucketsResult"]["Buckets"]["Bucket"];
        assert_eq!(buckets[0]["Name"], "one");
        assert_eq!(buckets[1]["Name"], "two");
    }
}
