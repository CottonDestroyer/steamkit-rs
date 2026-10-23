//! Turns a decoded body into the tree of nodes the analyzer shows.
//!
//!
//! Rust has no runtime reflection, so the fields are pulled out of the
//! decoded message's `Debug` output, which has the same names.

use serde::Serialize;

use crate::nethook::body::{Body, ProtobufBody, ProtobufField};

/// A node of the decoded message tree.
#[derive(Clone, Debug, Serialize)]
pub struct TreeNode {
    /// The label of the node.
    pub name: String,
    /// The value that is shown next to the label, if any.
    pub value: Option<String>,
    /// The children of the node.
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// Creates a node with a label and a value.
    pub fn new(name: impl Into<String>, value: Option<String>) -> Self {
        TreeNode {
            name: name.into(),
            value,
            children: Vec::new(),
        }
    }

    /// Creates a node with a label and children.
    pub fn with_children(name: impl Into<String>, children: Vec<TreeNode>) -> Self {
        TreeNode {
            name: name.into(),
            value: None,
            children,
        }
    }
}

/// Builds the node of a value that is shown through its `Debug` representation.
pub fn debug_node(name: &str, debug: &str, show_unset_fields: bool) -> TreeNode {
    let mut node = TreeNode::new(name.to_owned(), None);
    node.children = fields_of(debug, show_unset_fields);
    node
}

/// Builds the node of a decoded protobuf message.
pub fn protobuf_node(
    type_name: &str,
    message: &dyn ProtobufBody,
    show_unset_fields: bool,
) -> TreeNode {
    let mut node = TreeNode::new(type_name.to_owned(), None);
    node.children = fields_of(&format!("{message:?}"), show_unset_fields);
    node
}

/// Builds the node of a decoded body.
pub fn body_node(body: &Body, show_unset_fields: bool) -> TreeNode {
    match body {
        Body::Protobuf {
            type_name,
            message,
            ..
        } => {
            let mut node = TreeNode::new(type_name.clone(), None);
            node.children = fields_of(&format!("{message:?}"), show_unset_fields);
            node
        }
        Body::Serializable {
            type_name,
            message,
        } => {
            let mut node = TreeNode::new(type_name.clone(), None);
            node.children = fields_of(&format!("{message:?}"), show_unset_fields);
            node
        }
        Body::Fields(fields) => TreeNode::with_children("Protobuf", field_nodes(fields)),
        Body::Unknown(message) => {
            if message.is_empty() {
                TreeNode::new("<null>", None)
            } else {
                TreeNode::new(message.clone(), None)
            }
        }
    }
}

/// Builds the nodes of the generic protobuf field list.
fn field_nodes(fields: &[ProtobufField]) -> Vec<TreeNode> {
    fields
        .iter()
        .map(|field| {
            let values = field
                .values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(", ");

            TreeNode::new(
                format!("[ {} ]", field.number),
                Some(format!("{}: {}", field.wire_type.as_str(), values)),
            )
        })
        .collect()
}

/// Builds the child nodes of the fields of a decoded message.
fn fields_of(debug: &str, show_unset_fields: bool) -> Vec<TreeNode> {
    let (_, fields) = match parse_struct(debug) {
        Some(parsed) => parsed,
        None => return Vec::new(),
    };

    fields
        .iter()
        .filter(|(_, value)| show_unset_fields || !is_unset(value))
        .map(|(name, value)| field_node(name, value))
        .collect()
}

/// Whether a field of a decoded message was not set on the wire.
fn is_unset(value: &str) -> bool {
    value == "None" || value == "[]" || value == "vec![]"
}

/// Builds the node of a single field of a decoded message.
fn field_node(name: &str, value: &str) -> TreeNode {
    let inner = match unwrap_option(value) {
        Some(inner) => inner,
        None => return TreeNode::new(name, Some(value.to_owned())),
    };

    if let Some(elements) = parse_list(inner) {
        let mut children = Vec::new();
        for (index, element) in elements.iter().enumerate() {
            children.push(field_node(&format!("[ {index} ]"), element));
        }

        if children.iter().all(|child| child.children.is_empty()) {
            return TreeNode::new(name, Some(value.to_owned()));
        }

        return TreeNode {
            name: name.to_owned(),
            value: Some(format!("[ {} ]", elements.len())),
            children,
        };
    }

    if let Some((type_name, fields)) = parse_struct(inner) {
        let mut node = TreeNode::new(name, Some(type_name));
        node.children = fields
            .iter()
            .map(|(field_name, field_value)| field_node(field_name, field_value))
            .collect();
        return node;
    }

    TreeNode::new(name, Some(value.to_owned()))
}

/// The value of a field without its `Option` wrapper.
fn unwrap_option(value: &str) -> Option<&str> {
    if value == "None" {
        return None;
    }

    if value.starts_with("Some(") && value.ends_with(')') {
        return Some(&value["Some(".len()..value.len() - 1]);
    }

    Some(value)
}

/// Splits a `Debug` struct into its type name and its fields.
fn parse_struct(text: &str) -> Option<(String, Vec<(String, String)>)> {
    let text = text.trim();
    let open = text.find('{')?;
    let close = text.rfind('}')?;
    if close < open {
        return None;
    }

    let name = text[..open].trim().to_owned();
    let body = &text[open + 1..close];

    let mut fields = Vec::new();
    for part in split_top_level(body) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let colon = match find_top_level_colon(part) {
            Some(colon) => colon,
            None => continue,
        };

        fields.push((
            part[..colon].trim().to_owned(),
            part[colon + 1..].trim().to_owned(),
        ));
    }

    Some((name, fields))
}

/// Splits a list of values.
fn parse_list(text: &str) -> Option<Vec<String>> {
    let text = text.trim();

    // A repeated field of a `prost` message is printed as `vec![...]`.
    if let Some(rest) = text.strip_prefix("vec![") {
        if let Some(inner) = rest.strip_suffix(']') {
            return Some(
                split_top_level(inner)
                    .iter()
                    .map(|part| part.trim().to_owned())
                    .collect(),
            );
        }
    }

    if text.len() < 2 || !text.starts_with('[') || !text.ends_with(']') {
        return None;
    }

    Some(
        split_top_level(&text[1..text.len() - 1])
            .iter()
            .map(|part| part.trim().to_owned())
            .collect(),
    )
}

/// Splits a comma separated list at the top nesting level.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            ',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    parts.push(&text[start..]);
    parts
}

/// The position of the colon that separates a field name from its value.
fn find_top_level_colon(text: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = (depth - 1).max(0),
            ':' if depth == 0 => return Some(index),
            _ => {}
        }
    }

    None
}
