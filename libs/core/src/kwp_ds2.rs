use crate::error::{HandosError, Result};
use crate::protocol::{xor_checksum, TESTER_ADDRESS};

/// Raw KWP2000 packet (ISO 14230 / BMW D-CAN & K-Line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KwpPacket {
    pub target: u8,
    pub source: u8,
    pub payload: Vec<u8>,
}

impl KwpPacket {
    pub fn new(target: u8, payload: Vec<u8>) -> Self {
        Self {
            target,
            source: TESTER_ADDRESS,
            payload,
        }
    }

    /// Encodes as ISO 14230 physical-addressing frame with XOR checksum:
    /// `[0x80 | len_or_0][target][source][optional_len][payload...][checksum]`
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.payload.is_empty() || self.payload.len() > 255 {
            return Err(HandosError::Protocol(format!(
                "invalid KWP2000 payload length: {}",
                self.payload.len()
            )));
        }

        let mut buf = Vec::with_capacity(self.payload.len() + 5);
        if self.payload.len() <= 63 {
            buf.push(0x80 | (self.payload.len() as u8));
            buf.push(self.target);
            buf.push(self.source);
        } else {
            buf.push(0x80);
            buf.push(self.target);
            buf.push(self.source);
            buf.push(self.payload.len() as u8);
        }
        buf.extend_from_slice(&self.payload);
        let csum = xor_checksum(&buf);
        buf.push(csum);
        Ok(buf)
    }

    /// Decodes an ISO 14230 KWP2000 frame and verifies its XOR checksum.
    pub fn decode(raw: &[u8]) -> Result<Self> {
        if raw.len() < 5 {
            return Err(HandosError::Protocol("KWP2000 frame too short".to_string()));
        }
        let expected_csum = raw[raw.len() - 1];
        let actual_csum = xor_checksum(&raw[..raw.len() - 1]);
        if expected_csum != actual_csum {
            return Err(HandosError::Protocol(format!(
                "KWP2000 checksum mismatch: expected 0x{expected_csum:02X}, got 0x{actual_csum:02X}"
            )));
        }

        let fmt = raw[0];
        let target = raw[1];
        let source = raw[2];
        let inline_len = (fmt & 0x3F) as usize;
        let (header_len, payload_len) = if inline_len > 0 {
            (3, inline_len)
        } else {
            (4, raw[3] as usize)
        };

        if raw.len() != header_len + payload_len + 1 {
            return Err(HandosError::Protocol(format!(
                "KWP2000 length mismatch: header={header_len}, payload={payload_len}, total={}",
                raw.len()
            )));
        }

        Ok(Self {
            target,
            source,
            payload: raw[header_len..header_len + payload_len].to_vec(),
        })
    }
}

/// Raw DS2 packet (legacy BMW K-Line used on E46 LSZ, GM5, IKE, MS42/MS43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ds2Packet {
    pub address: u8,
    pub payload: Vec<u8>,
}

impl Ds2Packet {
    pub fn new(address: u8, payload: Vec<u8>) -> Self {
        Self { address, payload }
    }

    /// Encodes DS2 frame: `[address][total_len][payload...][xor_checksum]`.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let total_len = self.payload.len() + 3;
        if total_len > 255 || self.payload.is_empty() {
            return Err(HandosError::Protocol(format!(
                "invalid DS2 payload length: {}",
                self.payload.len()
            )));
        }
        let mut buf = Vec::with_capacity(total_len);
        buf.push(self.address);
        buf.push(total_len as u8);
        buf.extend_from_slice(&self.payload);
        let csum = xor_checksum(&buf);
        buf.push(csum);
        Ok(buf)
    }

    pub fn decode(raw: &[u8]) -> Result<Self> {
        if raw.len() < 4 {
            return Err(HandosError::Protocol("DS2 frame too short".to_string()));
        }
        let total_len = raw[1] as usize;
        if raw.len() != total_len {
            return Err(HandosError::Protocol(format!(
                "DS2 length mismatch: header says {total_len}, got {}",
                raw.len()
            )));
        }
        let expected_csum = raw[raw.len() - 1];
        let actual_csum = xor_checksum(&raw[..raw.len() - 1]);
        if expected_csum != actual_csum {
            return Err(HandosError::Protocol(format!(
                "DS2 checksum mismatch: expected 0x{expected_csum:02X}, got 0x{actual_csum:02X}"
            )));
        }
        Ok(Self {
            address: raw[0],
            payload: raw[2..raw.len() - 1].to_vec(),
        })
    }
}

/// Standard BMW KWP2000 diagnostic service builders.
pub mod kwp_jobs {
    use super::KwpPacket;

    pub fn start_diagnostic_session(ecu_addr: u8, session_mode: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x10, session_mode])
    }

    pub fn read_vin(ecu_addr: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x1A, 0x90])
    }

    pub fn read_ecu_ident(ecu_addr: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x1A, 0x80])
    }

    pub fn read_coding_block(ecu_addr: u8, local_id: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x21, local_id])
    }

    pub fn write_coding_block(ecu_addr: u8, local_id: u8, data: &[u8]) -> KwpPacket {
        let mut payload = Vec::with_capacity(2 + data.len());
        payload.push(0x3B);
        payload.push(local_id);
        payload.extend_from_slice(data);
        KwpPacket::new(ecu_addr, payload)
    }

    pub fn read_dtcs(ecu_addr: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x18, 0x00, 0xFF, 0x00])
    }

    pub fn clear_dtcs(ecu_addr: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x14, 0xFF, 0x00])
    }

    pub fn tester_present(ecu_addr: u8) -> KwpPacket {
        KwpPacket::new(ecu_addr, vec![0x3E, 0x01])
    }
}
