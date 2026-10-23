//! Decoding of captured Steam messages.
//!
//! A captured packet is a four byte message number, a header, a body and,
//! for some messages, a payload after the body. Protobuf messages have a
//! protobuf header, the channel encryption messages have the plain header,
//! and everything else has the extended client header.

use prost::Message as _;
use steamkit::base::generated::steam_language::EMsg;
use steamkit::base::generated::steam_language_internal::{
    ExtendedClientMsgHdr, MsgGCHdr, MsgGCHdrProtoBuf, MsgHdr, MsgHdrProtoBuf,
    SteamSerializableDeserialize,
};
use steamkit::base::msg_base::Reader;
use steamkit::util::msg_util::MsgUtil;
use steamkit_protos::steam::{CMsgClientServiceMethodLegacy, CMsgClientServiceMethodLegacyResponse, CMsgGcClient};

use crate::nethook::body::Body;
use crate::nethook::error::{NetHookError, NetHookResult};
use crate::nethook::emsg_extensions;
use crate::nethook::message_type_finder;
use crate::nethook::proto_registry::{self, ProtobufDecoder};
use crate::nethook::protobuf_field_reader;

/// The header of a captured message.
#[derive(Clone, Debug)]
pub enum Header {
    /// The plain Steam message header.
    MsgHdr(MsgHdr),
    /// The extended Steam client message header.
    ExtendedClientMsgHdr(ExtendedClientMsgHdr),
    /// The protobuf Steam message header.
    MsgHdrProtoBuf(MsgHdrProtoBuf),
    /// The plain game coordinator message header.
    MsgGCHdr(MsgGCHdr),
    /// The protobuf game coordinator message header.
    MsgGCHdrProtoBuf(MsgGCHdrProtoBuf),
}

/// A captured message.
#[derive(Debug)]
pub struct ReadMessage {
    /// The raw message number of the packet, including the protobuf flag.
    pub raw_emsg: u32,
    /// The header of the message.
    pub header: Header,
    /// The body of the message.
    pub body: Body,
    /// The bytes that follow the body.
    pub payload: Vec<u8>,
}

/// The name of the unified messaging method the message carries.
pub fn target_job_name(header: &Header) -> String {
    match header {
        Header::MsgHdrProtoBuf(header) => header
            .proto
            .target_job_name
            .clone()
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Reads a captured packet.
pub fn read_message(bytes: &[u8]) -> NetHookResult<ReadMessage> {
    if bytes.len() < 4 {
        return Err(NetHookError::malformed(
            "capture is shorter than a message number",
        ));
    }

    let raw_emsg = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

    // The protobuf header embeds the message number, so its reader starts at the
    // beginning of the capture; the other headers follow the message number.
    let (header, body_start) = if MsgUtil::is_proto_buf(raw_emsg) {
        let mut header_reader = Reader::new(bytes);
        let header = read_header(raw_emsg, &mut header_reader)?;
        (header, header_reader.position())
    } else {
        let mut header_reader = Reader::new(&bytes[4..]);
        let header = read_header(raw_emsg, &mut header_reader)?;
        (header, 4 + header_reader.position())
    };

    let body_bytes = &bytes[body_start..];

    let (body, consumed) = read_body(raw_emsg, &header, body_bytes)?;
    let payload = body_bytes[consumed.min(body_bytes.len())..].to_vec();

    Ok(ReadMessage {
        raw_emsg,
        header,
        body,
        payload,
    })
}

/// Reads the header of a captured packet.
pub fn read_header(raw_emsg: u32, reader: &mut Reader<'_>) -> NetHookResult<Header> {
    if MsgUtil::is_proto_buf(raw_emsg) {
        return Ok(Header::MsgHdrProtoBuf(MsgHdrProtoBuf::read(reader)?));
    }

    let channel_encrypt = raw_emsg == EMsg::ChannelEncryptRequest.value() as u32
        || raw_emsg == EMsg::ChannelEncryptResponse.value() as u32
        || raw_emsg == EMsg::ChannelEncryptResult.value() as u32;

    if channel_encrypt {
        return Ok(Header::MsgHdr(MsgHdr::read(reader)?));
    }

    Ok(Header::ExtendedClientMsgHdr(ExtendedClientMsgHdr::read(
        reader,
    )?))
}

/// Reads the body of a captured packet and reports how many bytes it used.
fn read_body(
    raw_emsg: u32,
    header: &Header,
    body_bytes: &[u8],
) -> NetHookResult<(Body, usize)> {
    match MsgUtil::get_msg(raw_emsg) {
        Some(EMsg::ServiceMethod)
        | Some(EMsg::ServiceMethodSendToClient)
        | Some(EMsg::ServiceMethodCallFromClient)
        | Some(EMsg::ServiceMethodCallFromClientNonAuthed) => {
            let name = target_job_name(header);
            let types = proto_registry::service_method_types(&name).ok_or_else(|| {
                NetHookError::malformed(format!("unknown service method '{name}'"))
            })?;
            decode_protobuf_body(body_bytes, types.request_type, types.request_decoder)
        }
        Some(EMsg::ServiceMethodResponse) => {
            let name = target_job_name(header);
            let types = proto_registry::service_method_types(&name).ok_or_else(|| {
                NetHookError::malformed(format!("unknown service method '{name}'"))
            })?;
            decode_protobuf_body(body_bytes, types.response_type, types.response_decoder)
        }
        Some(EMsg::ClientServiceMethodLegacy) => {
            let legacy = CMsgClientServiceMethodLegacy::decode(body_bytes)
                .map_err(|error| NetHookError::protobuf(error.to_string()))?;
            let name = legacy.method_name.clone().unwrap_or_default();
            let serialized = legacy.serialized_method.clone().unwrap_or_default();
            let types = proto_registry::service_method_types(&name).ok_or_else(|| {
                NetHookError::malformed(format!("unknown service method '{name}'"))
            })?;
            let (body, _) = decode_protobuf_body(
                &serialized,
                types.request_type,
                types.request_decoder,
            )?;
            Ok((body, body_bytes.len()))
        }
        Some(EMsg::ClientServiceMethodLegacyResponse) => {
            let legacy = CMsgClientServiceMethodLegacyResponse::decode(body_bytes)
                .map_err(|error| NetHookError::protobuf(error.to_string()))?;
            let name = legacy.method_name.clone().unwrap_or_default();
            let serialized = legacy.serialized_method_response.clone().unwrap_or_default();
            let types = proto_registry::service_method_types(&name).ok_or_else(|| {
                NetHookError::malformed(format!("unknown service method '{name}'"))
            })?;
            let (body, _) = decode_protobuf_body(
                &serialized,
                types.response_type,
                types.response_decoder,
            )?;
            Ok((body, body_bytes.len()))
        }
        _ => read_message_body(raw_emsg, body_bytes),
    }
}

/// Reads the body of a message that is not a unified messaging message.
fn read_message_body(raw_emsg: u32, body_bytes: &[u8]) -> NetHookResult<(Body, usize)> {
    if let Some(emsg) = MsgUtil::get_msg(raw_emsg) {
        if let Some(protobuf_type) = message_type_finder::protobuf_message_body_type(emsg) {
            return decode_protobuf_body(body_bytes, protobuf_type.name, protobuf_type.decoder);
        }

        if let Some(serializable_type) =
            message_type_finder::non_protobuf_message_body_type(emsg)
        {
            let mut reader = Reader::new(body_bytes);
            let message = (serializable_type.decoder)(&mut reader)?;
            return Ok((
                Body::Serializable {
                    type_name: serializable_type.name.to_owned(),
                    message,
                },
                reader.position(),
            ));
        }
    }

    if MsgUtil::is_proto_buf(raw_emsg) {
        let fields = protobuf_field_reader::read_protobuf(body_bytes)?;
        if fields.is_empty() {
            return Ok((Body::Unknown(String::new()), body_bytes.len()));
        }
        return Ok((Body::Fields(fields), body_bytes.len()));
    }

    Ok((Body::Unknown(String::new()), body_bytes.len()))
}

/// Decodes a protobuf body and reports how many bytes it used.
///
/// A protobuf body always runs to the end of the capture, so protobuf
/// messages never have a payload.
fn decode_protobuf_body(
    bytes: &[u8],
    type_name: &str,
    decoder: ProtobufDecoder,
) -> NetHookResult<(Body, usize)> {
    let message = decoder(bytes).map_err(|error| NetHookError::protobuf(error.to_string()))?;

    Ok((
        Body::Protobuf {
            type_name: type_name.to_owned(),
            bytes: bytes.to_vec(),
            message,
        },
        bytes.len(),
    ))
}

/// The name of the message inside a captured packet.
///
/// This is the unified messaging method or the game coordinator message the
/// packet carries, if any.
pub fn inner_message_name(emsg: EMsg, bytes: &[u8]) -> Option<String> {
    match emsg {
        EMsg::ClientToGC | EMsg::ClientFromGC => {
            let message = read_message(bytes).ok()?;
            let gc = message.body.decode_as::<CMsgGcClient>()?;
            let gc_emsg = gc.msgtype.unwrap_or_default();
            let app_id = gc.appid.unwrap_or_default();
            let name = emsg_extensions::gc_message_name(gc_emsg, app_id);
            Some(
                name.strip_prefix("k_EMsg")
                    .unwrap_or(name.as_str())
                    .to_owned(),
            )
        }
        EMsg::ServiceMethod
        | EMsg::ServiceMethodSendToClient
        | EMsg::ServiceMethodCallFromClient
        | EMsg::ServiceMethodCallFromClientNonAuthed
        | EMsg::ServiceMethodResponse => {
            if bytes.len() < 4 {
                return None;
            }
            let raw_emsg = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let mut reader = Reader::new(&bytes[4..]);
            let header = read_header(raw_emsg, &mut reader).ok()?;
            Some(target_job_name(&header))
        }
        EMsg::ClientServiceMethodLegacy => {
            let body_bytes = body_bytes_after_header(bytes)?;
            let legacy = CMsgClientServiceMethodLegacy::decode(body_bytes).ok()?;
            Some(legacy.method_name.unwrap_or_default())
        }
        EMsg::ClientServiceMethodLegacyResponse => {
            let body_bytes = body_bytes_after_header(bytes)?;
            let legacy = CMsgClientServiceMethodLegacyResponse::decode(body_bytes).ok()?;
            Some(legacy.method_name.unwrap_or_default())
        }
        _ => Some(String::new()),
    }
}

/// The display label for a message number: `EMsg <name> (<number>)`, plus
/// ` (Non-Protobuf)` for messages that aren't protobuf backed.
pub fn emsg_to_string_name(raw_emsg: u32) -> String {
    let name = match MsgUtil::get_msg(raw_emsg) {
        Some(emsg) => format!("EMsg {emsg:?} ({})", emsg.value()),
        None => {
            let value = raw_emsg & !0x8000_0000;
            format!("EMsg {value} ({value})")
        }
    };

    if MsgUtil::is_proto_buf(raw_emsg) {
        name
    } else {
        format!("{name} (Non-Protobuf)")
    }
}

/// The bytes of a captured packet that follow its header.
fn body_bytes_after_header(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 4 {
        return None;
    }

    let raw_emsg = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let mut reader = Reader::new(&bytes[4..]);
    read_header(raw_emsg, &mut reader).ok()?;
    Some(&bytes[4 + reader.position()..])
}
