//! IP address and endpoint helpers.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use steamkit_protos::steam::{c_msg_ip_address, CMsgIpAddress};

use crate::base::generated::steam_language_internal::MsgClientLogon;
use crate::error::SteamError;

/// Helpers for dealing with IP addresses and endpoints.
#[derive(Debug)]
pub struct NetHelpers;

impl NetHelpers {
    /// The local IP of a connected socket, from its local endpoint. An error if
    /// it's unspecified.
    pub fn get_local_ip(local_end_point: Option<SocketAddr>) -> Result<IpAddr, SteamError> {
        match local_end_point {
            Some(end_point) if !end_point.ip().is_unspecified() => Ok(end_point.ip()),
            _ => Err(SteamError::Connection("Socket not connected".to_string())),
        }
    }

    /// Converts a packed IPv4 address (network byte order) to an [`IpAddr`].
    /// The inverse of [`NetHelpers::get_ip_address_as_uint`].
    pub fn get_ip_address(ip_addr: u32) -> IpAddr {
        IpAddr::V4(Ipv4Addr::from(ip_addr))
    }

    /// Packs an IPv4 address into a `u32` (network byte order). The inverse of
    /// [`NetHelpers::get_ip_address`]. IPv6 is an error.
    pub fn get_ip_address_as_uint(ip_addr: &IpAddr) -> Result<u32, SteamError> {
        match ip_addr {
            IpAddr::V4(address) => Ok(u32::from_be_bytes(address.octets())),
            IpAddr::V6(_) => Err(SteamError::InvalidArgument(
                "GetIPAddressAsUInt only works with IPv4 addresses.".to_string(),
            )),
        }
    }

    /// Converts a `CMsgIpAddress` to an [`IpAddr`], preferring the IPv6 form
    /// when it's there. An IPv6 address that isn't 16 bytes gives `None`.
    pub fn get_ip_address_from_msg(ip_addr: &CMsgIpAddress) -> Option<IpAddr> {
        match &ip_addr.ip {
            Some(c_msg_ip_address::Ip::V6(bytes)) => {
                let octets: [u8; 16] = bytes.as_slice().try_into().ok()?;
                Some(IpAddr::V6(Ipv6Addr::from(octets)))
            }
            Some(c_msg_ip_address::Ip::V4(value)) => {
                Some(IpAddr::V4(Ipv4Addr::from(*value)))
            }
            None => None,
        }
    }

    /// Converts an [`IpAddr`] to a `CMsgIpAddress`.
    pub fn get_msg_ip_address(ip_addr: &IpAddr) -> CMsgIpAddress {
        let ip = match ip_addr {
            IpAddr::V4(address) => {
                c_msg_ip_address::Ip::V4(u32::from_be_bytes(address.octets()))
            }
            IpAddr::V6(address) => c_msg_ip_address::Ip::V6(address.octets().to_vec()),
        };

        CMsgIpAddress { ip: Some(ip) }
    }

    /// Obfuscates a private IP address in place: IPv6 is XORed with the
    /// repeating key `0D F0 AD BA`, IPv4 with `MsgClientLogon::OBFUSCATION_MASK`.
    pub fn obfuscate_private_ip(msg_ip_address: &mut CMsgIpAddress) {
        match &mut msg_ip_address.ip {
            Some(c_msg_ip_address::Ip::V6(bytes)) => {
                const KEY: [u8; 4] = [0x0D, 0xF0, 0xAD, 0xBA];

                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte ^= KEY[index % KEY.len()];
                }
            }
            Some(c_msg_ip_address::Ip::V4(value)) => {
                *value ^= MsgClientLogon::OBFUSCATION_MASK;
            }
            None => {}
        }
    }

    /// Parses an `address:port` endpoint. Port 0 is rejected.
    pub fn try_parse_ip_end_point(string_value: &str) -> Option<SocketAddr> {
        let end_point = string_value.parse::<SocketAddr>().ok()?;

        if end_point.port() == 0 {
            return None;
        }

        Some(end_point)
    }

    /// The host part of an endpoint.
    pub fn extract_endpoint_host(end_point: &SocketAddr) -> String {
        end_point.ip().to_string()
    }

    /// The port of an endpoint.
    pub fn extract_endpoint_port(end_point: &SocketAddr) -> u16 {
        end_point.port()
    }
}
