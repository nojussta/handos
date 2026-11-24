use crate::error::{HandosError, Result};
use std::io::{Read, Write};

/// Default MHD Universal WiFi adapter address and port.
pub const DEFAULT_MHD_WIFI_HOST: &str = "192.168.4.1";
pub const DEFAULT_MHD_WIFI_PORT: u16 = 6801;

/// Tester (Handos app) diagnostic address.
pub const TESTER_ADDRESS: u8 = 0xF4;
/// Gateway / Adapter broadcast address.
pub const GATEWAY_ADDRESS: u8 = 0x00;

/// Payload types for BMW ENET / MHD WiFi TCP framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PayloadType {
    Diagnostic = 0x0001,
    AdapterControl = 0x0002,
    Error = 0x00FF,
}

impl TryFrom<u16> for PayloadType {
    type Error = HandosError;

    fn try_from(value: u16) -> Result<Self> {
        match value {
            0x0001 => Ok(Self::Diagnostic),
            0x0002 => Ok(Self::AdapterControl),
            0x00FF => Ok(Self::Error),
            other => Err(HandosError::Protocol(format!(
                "unknown frame payload type: 0x{other:04X}"
            ))),
        }
    }
}

/// KWP2000 / UDS-style Service IDs used by Handos.
pub mod sid {
    pub const ADAPTER_SCAN: u8 = 0x01;
    pub const READ_ECU_ID: u8 = 0x1A;
    pub const READ_CODING_DATA: u8 = 0x21;
    pub const WRITE_CODING_DATA: u8 = 0x3B;
    pub const READ_DTC: u8 = 0x18;
    pub const CLEAR_DTC: u8 = 0x14;
    pub const NEGATIVE_RESPONSE: u8 = 0x7F;
}

/// Wire frame exchanged over the MHD WiFi TCP bridge.
///
/// Layout:
/// - `4 bytes` big-endian length (`2 + 1 + 1 + payload.len()`)
/// - `2 bytes` big-endian payload type
/// - `1 byte` source address
/// - `1 byte` target address
/// - `N bytes` payload
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticFrame {
    pub payload_type: PayloadType,
    pub source: u8,
    pub target: u8,
    pub payload: Vec<u8>,
}

impl DiagnosticFrame {
    pub fn new(payload_type: PayloadType, source: u8, target: u8, payload: Vec<u8>) -> Self {
        Self {
            payload_type,
            source,
            target,
            payload,
        }
    }

    pub fn diagnostic(target: u8, payload: Vec<u8>) -> Self {
        Self::new(PayloadType::Diagnostic, TESTER_ADDRESS, target, payload)
    }

    pub fn adapter_control(payload: Vec<u8>) -> Self {
        Self::new(
            PayloadType::AdapterControl,
            TESTER_ADDRESS,
            GATEWAY_ADDRESS,
            payload,
        )
    }

    pub fn encode(&self) -> Vec<u8> {
        let body_len = (4 + self.payload.len()) as u32;
        let mut buf = Vec::with_capacity(4 + body_len as usize);
        buf.extend_from_slice(&body_len.to_be_bytes());
        buf.extend_from_slice(&(self.payload_type as u16).to_be_bytes());
        buf.push(self.source);
        buf.push(self.target);
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<()> {
        let encoded = self.encode();
        writer.write_all(&encoded)?;
        writer.flush()?;
        Ok(())
    }

    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let body_len = u32::from_be_bytes(len_buf) as usize;
        if !(4..=4 * 1024 * 1024).contains(&body_len) {
            return Err(HandosError::Protocol(format!(
                "invalid frame body length: {body_len}"
            )));
        }

        let mut body = vec![0u8; body_len];
        reader.read_exact(&mut body)?;

        let ptype = u16::from_be_bytes([body[0], body[1]]);
        let payload_type = PayloadType::try_from(ptype)?;
        let source = body[2];
        let target = body[3];
        let payload = body[4..].to_vec();

        Ok(Self {
            payload_type,
            source,
            target,
            payload,
        })
    }

    /// Checks whether a diagnostic response is a KWP/UDS negative response (`0x7F`).
    pub fn check_positive(&self, ecu_name: &str, expected_sid: u8) -> Result<&[u8]> {
        if self.payload.is_empty() {
            return Err(HandosError::Protocol(format!(
                "empty response from {ecu_name}"
            )));
        }
        if self.payload[0] == sid::NEGATIVE_RESPONSE {
            let nrc = self.payload.get(2).copied().unwrap_or(0x00);
            return Err(HandosError::NegativeResponse {
                ecu: ecu_name.to_string(),
                nrc,
                message: nrc_description(nrc).to_string(),
            });
        }
        let expected_positive = expected_sid.wrapping_add(0x40);
        if self.payload[0] != expected_positive {
            return Err(HandosError::Protocol(format!(
                "unexpected SID 0x{:02X} from {ecu_name} (expected 0x{expected_positive:02X})",
                self.payload[0]
            )));
        }
        Ok(&self.payload[1..])
    }
}

/// Computes standard BMW KWP2000/DS2 XOR checksum over a byte slice.
pub fn xor_checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |acc, &b| acc ^ b)
}

pub fn nrc_description(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "generalReject",
        0x11 => "serviceNotSupported",
        0x12 => "subFunctionNotSupported",
        0x13 => "incorrectMessageLengthOrInvalidFormat",
        0x22 => "conditionsNotCorrect",
        0x31 => "requestOutOfRange",
        0x33 => "securityAccessDenied",
        0x78 => "requestCorrectlyReceivedResponsePending",
        _ => "unknownNegativeResponse",
    }
}
