//! Multi messages, which carry several Steam messages in their body.

use std::io::Read;

use flate2::read::GzDecoder;
use steamkit_protos::steam::CMsgMulti;

use crate::nethook::body::Body;
use crate::nethook::message_reading;
use crate::nethook::render::{body_node, debug_node, TreeNode};
use crate::nethook::specializations::{ExtraObject, Specialization, SpecializationContext};

/// Decodes the messages a multiplexed message carries.
pub struct MultiPacketSpecialization;

impl Specialization for MultiPacketSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let multi = match body.decode_as::<CMsgMulti>() {
            Some(multi) => multi,
            None => return Vec::new(),
        };

        let message_body = multi.message_body.clone().unwrap_or_default();
        let size_unzipped = multi.size_unzipped.unwrap_or_default();

        let data = if size_unzipped > 0 {
            let mut decoder = GzDecoder::new(message_body.as_slice());
            let mut decompressed = Vec::new();
            if decoder.read_to_end(&mut decompressed).is_err() {
                return Vec::new();
            }
            decompressed
        } else {
            message_body
        };

        let mut objects = Vec::new();
        let mut offset = 0usize;

        while offset + 4 <= data.len() {
            let length = i32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);

            offset += 4;

            if length < 0 {
                break;
            }

            let length = (length as usize).min(data.len() - offset);
            let inner = &data[offset..offset + length];
            offset += length;

            if let Ok(message) = message_reading::read_message(inner) {
                let name = message_reading::emsg_to_string_name(message.raw_emsg);

                let mut node = TreeNode::new(name.clone(), None);
                node.children.push(debug_node(
                    "Header",
                    &format!("{:?}", message.header),
                    context.show_unset_fields,
                ));
                node.children.push(body_node(&message.body, context.show_unset_fields));

                if !message.payload.is_empty() {
                    node.children.push(TreeNode::new(
                        "Payload",
                        Some(hex::encode(&message.payload).to_uppercase()),
                    ));
                }

                objects.push(ExtraObject::new(name, node));
            }
        }

        objects
    }
}
