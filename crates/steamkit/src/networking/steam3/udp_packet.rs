//! Steam UDP packets, plus the packet header and the two handshake payloads.
//!
//! # Wire format
//!
//! Every UDP datagram is a 36 byte little-endian header followed by the
//! payload:
//!
//! ```text
//! offset  size  field
//! 0       4     magic          (u32, always 0x31305356)
//! 4       2     payload_size   (u16)
//! 6       1     packet_type    (u8, EUdpPacketType)
//! 7       1     flags          (u8)
//! 8       4     source_conn_id (u32)
//! 12      4     dest_conn_id   (u32)
//! 16      4     seq_this       (u32)
//! 20      4     seq_ack        (u32)
//! 24      4     packets_in_msg (u32)
//! 28      4     msg_start_seq  (u32)
//! 32      4     msg_size       (u32)
//! 36      ..    payload
//! ```

use crate::base::generated::steam_language::EUdpPacketType;
use crate::error::SteamError;

/// The maximum payload size of a UDP packet: `0x4DC` (1244) bytes.
pub const MAX_PAYLOAD: usize = 0x4DC;

fn read_u32(data: &[u8], cursor: &mut usize) -> Result<u32, SteamError> {
    let bytes = read_bytes(data, cursor, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u16(data: &[u8], cursor: &mut usize) -> Result<u16, SteamError> {
    let bytes = read_bytes(data, cursor, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u8(data: &[u8], cursor: &mut usize) -> Result<u8, SteamError> {
    Ok(read_bytes(data, cursor, 1)?[0])
}

fn read_bytes<'a>(
    data: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], SteamError> {
    let end = cursor
        .checked_add(len)
        .ok_or_else(|| SteamError::Malformed("UDP header length overflow".to_string()))?;
    if end > data.len() {
        return Err(SteamError::Malformed(
            "UDP packet is too short to contain a header".to_string(),
        ));
    }
    let slice = &data[*cursor..end];
    *cursor = end;
    Ok(slice)
}

fn packet_type_from_u8(value: u8) -> EUdpPacketType {
    match value {
        0 => EUdpPacketType::Invalid,
        1 => EUdpPacketType::ChallengeReq,
        2 => EUdpPacketType::Challenge,
        3 => EUdpPacketType::Connect,
        4 => EUdpPacketType::Accept,
        5 => EUdpPacketType::Disconnect,
        6 => EUdpPacketType::Data,
        7 => EUdpPacketType::Datagram,
        8 => EUdpPacketType::Max,
        _ => EUdpPacketType::Invalid,
    }
}

fn packet_type_to_u8(value: EUdpPacketType) -> u8 {
    match value {
        EUdpPacketType::Invalid => 0,
        EUdpPacketType::ChallengeReq => 1,
        EUdpPacketType::Challenge => 2,
        EUdpPacketType::Connect => 3,
        EUdpPacketType::Accept => 4,
        EUdpPacketType::Disconnect => 5,
        EUdpPacketType::Data => 6,
        EUdpPacketType::Datagram => 7,
        EUdpPacketType::Max => 8,
    }
}

/// The header of a Steam UDP packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UdpHeader {
    /// Always [`UdpHeader::MAGIC`] for valid packets.
    pub magic: u32,
    /// The size of the payload that follows this header.
    pub payload_size: u16,
    /// The type of this packet.
    pub packet_type: EUdpPacketType,
    /// Protocol flags (unused).
    pub flags: u8,
    /// The connection id of the sender.
    pub source_conn_id: u32,
    /// The connection id of the receiver.
    pub dest_conn_id: u32,
    /// The sequence number of this packet.
    pub seq_this: u32,
    /// The highest sequence number received by the sender.
    pub seq_ack: u32,
    /// The number of packets this message is split into.
    pub packets_in_msg: u32,
    /// The sequence number of the first packet of this message.
    pub msg_start_seq: u32,
    /// The total size of the message this packet belongs to.
    pub msg_size: u32,
}

impl UdpHeader {
    /// The magic value that identifies a Steam UDP packet: `0x31305356`.
    pub const MAGIC: u32 = 0x31305356;

    /// The size of the serialized header in bytes.
    pub const SERIALIZED_SIZE: usize = 36;

    /// Creates a header with the default field values.
    pub fn new() -> Self {
        Self {
            magic: UdpHeader::MAGIC,
            payload_size: 0,
            packet_type: EUdpPacketType::Invalid,
            flags: 0,
            source_conn_id: 512,
            dest_conn_id: 0,
            seq_this: 0,
            seq_ack: 0,
            packets_in_msg: 0,
            msg_start_seq: 0,
            msg_size: 0,
        }
    }

    /// Serializes the header into its 36 byte little-endian wire form.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(UdpHeader::SERIALIZED_SIZE);
        out.extend_from_slice(&self.magic.to_le_bytes());
        out.extend_from_slice(&self.payload_size.to_le_bytes());
        out.push(packet_type_to_u8(self.packet_type));
        out.push(self.flags);
        out.extend_from_slice(&self.source_conn_id.to_le_bytes());
        out.extend_from_slice(&self.dest_conn_id.to_le_bytes());
        out.extend_from_slice(&self.seq_this.to_le_bytes());
        out.extend_from_slice(&self.seq_ack.to_le_bytes());
        out.extend_from_slice(&self.packets_in_msg.to_le_bytes());
        out.extend_from_slice(&self.msg_start_seq.to_le_bytes());
        out.extend_from_slice(&self.msg_size.to_le_bytes());
        out
    }

    /// Deserializes the header from the front of `data`. Fails as soon as the
    /// data runs out; fields read before that keep their new values.
    pub fn deserialize(&mut self, data: &[u8]) -> Result<(), SteamError> {
        let mut cursor = 0usize;

        self.magic = read_u32(data, &mut cursor)?;
        self.payload_size = read_u16(data, &mut cursor)?;
        let packet_type = read_u8(data, &mut cursor)?;
        self.packet_type = packet_type_from_u8(packet_type);
        self.flags = read_u8(data, &mut cursor)?;
        self.source_conn_id = read_u32(data, &mut cursor)?;
        self.dest_conn_id = read_u32(data, &mut cursor)?;
        self.seq_this = read_u32(data, &mut cursor)?;
        self.seq_ack = read_u32(data, &mut cursor)?;
        self.packets_in_msg = read_u32(data, &mut cursor)?;
        self.msg_start_seq = read_u32(data, &mut cursor)?;
        self.msg_size = read_u32(data, &mut cursor)?;

        Ok(())
    }
}

impl Default for UdpHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// The challenge the server sends during the UDP handshake.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChallengeData {
    /// The challenge value, to be XORed with [`ChallengeData::CHALLENGE_MASK`].
    pub challenge_value: u32,
    /// The server's load (unused).
    pub server_load: u32,
}

impl ChallengeData {
    /// The mask that is XORed with the challenge value.
    pub const CHALLENGE_MASK: u32 = 0xA426DF2B;

    /// Deserializes the challenge from the front of `data`.
    pub fn deserialize(data: &[u8]) -> Result<Self, SteamError> {
        if data.len() < 8 {
            return Err(SteamError::Malformed(
                "UDP challenge payload is shorter than 8 bytes".to_string(),
            ));
        }
        Ok(Self {
            challenge_value: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            server_load: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        })
    }

    /// Serializes the challenge into its 8 byte little-endian wire form.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        out.extend_from_slice(&self.challenge_value.to_le_bytes());
        out.extend_from_slice(&self.server_load.to_le_bytes());
        out
    }
}

/// The connect request sent during the UDP handshake.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConnectData {
    /// The masked challenge value.
    pub challenge_value: u32,
}

impl ConnectData {
    /// The mask that is XORed with the challenge value.
    pub const CHALLENGE_MASK: u32 = ChallengeData::CHALLENGE_MASK;

    /// Deserializes the connect data from the front of `data`.
    pub fn deserialize(data: &[u8]) -> Result<Self, SteamError> {
        if data.len() < 4 {
            return Err(SteamError::Malformed(
                "UDP connect payload is shorter than 4 bytes".to_string(),
            ));
        }
        Ok(Self {
            challenge_value: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        })
    }

    /// Serializes the connect data into its 4 byte little-endian wire form.
    pub fn serialize(&self) -> Vec<u8> {
        self.challenge_value.to_le_bytes().to_vec()
    }
}

/// A Steam UDP packet: a [`UdpHeader`] plus its payload.
#[derive(Clone, Debug)]
pub struct UdpPacket {
    /// The packet header.
    pub header: UdpHeader,
    payload: Option<Vec<u8>>,
}

impl UdpPacket {
    /// Creates a packet of the given type with no payload. The caller fills in
    /// the header.
    pub fn new(packet_type: EUdpPacketType) -> Self {
        let mut header = UdpHeader::new();
        header.packet_type = packet_type;

        Self {
            header,
            payload: Some(Vec::new()),
        }
    }

    /// Creates a packet of the given type carrying the given payload.
    pub fn with_payload(
        packet_type: EUdpPacketType,
        payload: Vec<u8>,
    ) -> Result<Self, SteamError> {
        let mut packet = Self::new(packet_type);
        packet.set_payload(payload)?;
        Ok(packet)
    }

    /// Creates a packet of the given type carrying `length` bytes taken from
    /// `source` starting at `offset`.
    pub fn with_payload_slice(
        packet_type: EUdpPacketType,
        source: &[u8],
        offset: usize,
        length: usize,
    ) -> Result<Self, SteamError> {
        let mut packet = Self::new(packet_type);
        packet.set_payload_slice(source, offset, length)?;
        Ok(packet)
    }

    /// Parses a received datagram.
    ///
    /// A malformed header or a bad magic gives a packet with no payload, so
    /// [`UdpPacket::is_valid`] returns `false`. SteamKit2 treats a truncated
    /// datagram as a valid empty packet instead; dropping it is safer.
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut header = UdpHeader::new();

        if header.deserialize(data).is_err() {
            return Self {
                header,
                payload: None,
            };
        }

        if header.magic != UdpHeader::MAGIC {
            return Self {
                header,
                payload: None,
            };
        }

        let length = header.payload_size as usize;
        if length > MAX_PAYLOAD {
            // SteamKit2 throws here; we just drop the packet.
            return Self {
                header,
                payload: None,
            };
        }

        // A datagram shorter than `payload_size` gives a zero-padded payload,
        // same as SteamKit2.
        let mut payload = vec![0u8; length];
        let available = data
            .len()
            .saturating_sub(UdpHeader::SERIALIZED_SIZE)
            .min(length);
        payload[..available].copy_from_slice(
            &data[UdpHeader::SERIALIZED_SIZE..UdpHeader::SERIALIZED_SIZE + available],
        );

        header.payload_size = length as u16;
        header.msg_size = length as u32;

        Self {
            header,
            payload: Some(payload),
        }
    }

    /// Whether this instance is valid.
    pub fn is_valid(&self) -> bool {
        self.header.magic == UdpHeader::MAGIC
            && self.header.payload_size as usize <= MAX_PAYLOAD
            && self.payload.is_some()
    }

    /// The payload of this packet, or an empty slice when the packet is invalid.
    pub fn payload(&self) -> &[u8] {
        self.payload.as_deref().unwrap_or(&[])
    }

    /// Replaces the payload and updates the header.
    pub fn set_payload(&mut self, payload: Vec<u8>) -> Result<(), SteamError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(SteamError::InvalidArgument(
                "Payload length exceeds 0x4DC maximum".to_string(),
            ));
        }

        self.header.payload_size = payload.len() as u16;
        self.header.msg_size = payload.len() as u32;
        self.payload = Some(payload);

        Ok(())
    }

    /// Replaces the payload with `length` bytes of `source` starting at
    /// `offset`, and updates the header.
    pub fn set_payload_slice(
        &mut self,
        source: &[u8],
        offset: usize,
        length: usize,
    ) -> Result<(), SteamError> {
        if length > MAX_PAYLOAD {
            return Err(SteamError::InvalidArgument(
                "Payload length exceeds 0x4DC maximum".to_string(),
            ));
        }

        let end = offset.saturating_add(length).min(source.len());
        let mut payload = vec![0u8; length];
        if end > offset {
            payload[..end - offset].copy_from_slice(&source[offset..end]);
        }

        self.header.payload_size = length as u16;
        self.header.msg_size = length as u32;
        self.payload = Some(payload);

        Ok(())
    }

    /// Serializes the packet.
    pub fn get_data(&self) -> Vec<u8> {
        let mut out = self.header.serialize();
        out.extend_from_slice(self.payload());
        out
    }
}
