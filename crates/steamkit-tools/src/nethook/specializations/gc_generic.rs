//! Game coordinator messages: decodes the header and body carried in the
//! payload.

use steamkit::base::generated::steam_language_internal::{
    MsgGCHdr, MsgGCHdrProtoBuf, SteamSerializableDeserialize,
};
use steamkit::base::msg_base::Reader;
use steamkit::util::msg_util::MsgUtil;
use steamkit_protos::steam::CMsgGcClient;

use crate::nethook::body::Body;
use crate::nethook::emsg_extensions;
use crate::nethook::message_type_finder;
use crate::nethook::protobuf_field_reader;
use crate::nethook::render::{body_node, debug_node, TreeNode};
use crate::nethook::specializations::{
    ExtraObject, GameCoordinatorSpecialization, Specialization, SpecializationContext,
};

/// Decodes game coordinator messages for the supported games.
pub struct GcGenericSpecialization {
    /// The game specific specializations.
    pub game_coordinator_specializations: Vec<Box<dyn GameCoordinatorSpecialization>>,
}

impl Specialization for GcGenericSpecialization {
    fn read_extra_objects(
        &self,
        body: &Body,
        context: &SpecializationContext<'_>,
    ) -> Vec<ExtraObject> {
        let gc_client = match body.decode_as::<CMsgGcClient>() {
            Some(gc_client) => gc_client,
            None => return Vec::new(),
        };

        let payload = gc_client.payload.clone().unwrap_or_default();
        let msg_type = gc_client.msgtype.unwrap_or_default();
        let app_id = gc_client.appid.unwrap_or_default();

        let (header, header_length) = match read_gc_header(msg_type, &payload) {
            Some(header) => header,
            None => return Vec::new(),
        };

        let gc_body = read_gc_body(msg_type, &payload[header_length..], app_id);

        let mut node = TreeNode::new("Game Coordinator Message", None);
        node.children.push(TreeNode::new(
            "emsg",
            Some(emsg_extensions::gc_message_name(msg_type, app_id)),
        ));
        node.children.push(debug_node(
            "header",
            &header,
            context.show_unset_fields,
        ));

        if let Some(gc_body) = gc_body.as_ref() {
            node.children.push(body_node(gc_body, context.show_unset_fields));
        }

        let mut objects = vec![ExtraObject::new("Game Coordinator Message", node)];

        if let Some(gc_body) = gc_body.as_ref() {
            for specialization in &self.game_coordinator_specializations {
                objects.extend(specialization.get_extra_objects(gc_body, app_id, context));
            }
        }

        objects
    }
}

/// Reads the header of a game coordinator message.
fn read_gc_header(raw_emsg: u32, payload: &[u8]) -> Option<(String, usize)> {
    let mut reader = Reader::new(payload);

    let header = if MsgUtil::is_proto_buf(raw_emsg) {
        format!("{:?}", MsgGCHdrProtoBuf::read(&mut reader).ok()?)
    } else {
        format!("{:?}", MsgGCHdr::read(&mut reader).ok()?)
    };

    Some((header, reader.position()))
}

/// Reads the body of a game coordinator message.
fn read_gc_body(raw_emsg: u32, body_bytes: &[u8], app_id: u32) -> Option<Body> {
    for candidate in message_type_finder::gc_message_body_type_candidates(raw_emsg, app_id) {
        if let Ok(message) = (candidate.decoder)(body_bytes) {
            return Some(Body::Protobuf {
                type_name: candidate.name.to_owned(),
                bytes: body_bytes.to_vec(),
                message,
            });
        }
    }

    let fields = protobuf_field_reader::read_protobuf(body_bytes).ok()?;
    if fields.is_empty() {
        return None;
    }

    Some(Body::Fields(fields))
}
