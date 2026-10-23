//! Builds the display tree of a captured packet: header, body, payload and
//! whatever the specializations add.

use crate::nethook::dump::NetHookDump;
use crate::nethook::error::NetHookError;
use crate::nethook::item::NetHookItem;
use crate::nethook::message_reading;
use crate::nethook::render::{body_node, debug_node, TreeNode};
use crate::nethook::specializations::{Specialization, SpecializationContext};

/// Builds the tree of a captured packet.
pub fn build_tree(
    item: &NetHookItem,
    dump: &NetHookDump,
    specializations: &[Box<dyn Specialization>],
    show_unset_fields: bool,
) -> TreeNode {
    match build_tree_core(item, dump, specializations, show_unset_fields) {
        Ok(node) => node,
        Err(error) => TreeNode::new(
            format!("NetHookError encountered whilst parsing item: {error}"),
            None,
        ),
    }
}

/// Builds the tree of a captured packet.
fn build_tree_core(
    item: &NetHookItem,
    dump: &NetHookDump,
    specializations: &[Box<dyn Specialization>],
    show_unset_fields: bool,
) -> Result<TreeNode, NetHookError> {
    let bytes = item.read_bytes()?;
    let message = message_reading::read_message(&bytes)?;

    let mut node = TreeNode::new(
        message_reading::emsg_to_string_name(message.raw_emsg),
        None,
    );

    node.children.push(debug_node(
        "Header",
        &format!("{:?}", message.header),
        show_unset_fields,
    ));

    let body = body_node(&message.body, show_unset_fields);
    let mut body_wrapper = TreeNode::new("Body", body.value.clone());
    body_wrapper.children = body.children;
    node.children.push(body_wrapper);

    if !message.payload.is_empty() {
        node.children.push(TreeNode::new(
            "Payload",
            Some(hex::encode(&message.payload).to_uppercase()),
        ));
    }

    let context = SpecializationContext {
        dump,
        show_unset_fields,
    };

    let mut to_examine = vec![message.body];

    while !to_examine.is_empty() {
        let mut objects = Vec::new();

        for body in &to_examine {
            for specialization in specializations {
                objects.extend(specialization.read_extra_objects(body, &context));
            }
        }

        if objects.is_empty() {
            break;
        }

        for object in objects {
            node.children.push(object.node);
            if let Some(body) = object.body {
                to_examine.push(body);
            }
        }
    }

    Ok(node)
}

/// Writes a tree as indented text.
pub fn write_tree(out: &mut String, node: &TreeNode, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }

    match node.value.as_ref() {
        Some(value) => {
            out.push_str(&node.name);
            out.push_str(": ");
            out.push_str(value);
        }
        None => out.push_str(&node.name),
    }

    out.push('\n');

    for child in &node.children {
        write_tree(out, child, depth + 1);
    }
}
