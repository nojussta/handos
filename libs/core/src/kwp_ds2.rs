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

/// ISO 15765-2 (ISO-TP) Flow Control status nibble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FlowStatus {
    ContinueToSend = 0x00,
    Wait = 0x01,
    Overflow = 0x02,
}

/// ISO 15765-2 (ISO-TP) 8-byte CAN frame segmentation and reassembly for D-CAN / ENET payloads.
pub struct IsoTpCodec;

impl IsoTpCodec {
    /// Segments an arbitrary diagnostic payload (up to 4095 bytes) into 8-byte CAN frames.
    pub fn segment(payload: &[u8]) -> Result<Vec<[u8; 8]>> {
        if payload.is_empty() || payload.len() > 4095 {
            return Err(HandosError::Protocol(format!(
                "invalid ISO-TP payload length: {}",
                payload.len()
            )));
        }

        if payload.len() <= 7 {
            let mut frame = [0u8; 8];
            frame[0] = payload.len() as u8;
            frame[1..=payload.len()].copy_from_slice(payload);
            return Ok(vec![frame]);
        }

        let mut frames = Vec::new();
        let total = payload.len();
        let mut ff = [0u8; 8];
        ff[0] = 0x10 | (((total >> 8) as u8) & 0x0F);
        ff[1] = (total & 0xFF) as u8;
        ff[2..8].copy_from_slice(&payload[..6]);
        frames.push(ff);

        let mut offset = 6;
        let mut sn: u8 = 1;
        while offset < total {
            let mut cf = [0u8; 8];
            cf[0] = 0x20 | (sn & 0x0F);
            let chunk = (total - offset).min(7);
            cf[1..=chunk].copy_from_slice(&payload[offset..offset + chunk]);
            frames.push(cf);
            offset += chunk;
            sn = (sn + 1) & 0x0F;
        }

        Ok(frames)
    }

    /// Builds an ISO-TP Flow Control (FC) frame.
    pub fn flow_control(status: FlowStatus, block_size: u8, st_min_ms: u8) -> [u8; 8] {
        [0x30 | (status as u8), block_size, st_min_ms, 0, 0, 0, 0, 0]
    }

    /// Reassembles a sequence of 8-byte ISO-TP CAN frames into the original diagnostic payload.
    pub fn reassemble(frames: &[[u8; 8]]) -> Result<Vec<u8>> {
        let first = frames
            .first()
            .ok_or_else(|| HandosError::Protocol("empty ISO-TP frame sequence".to_string()))?;
        let pci_type = first[0] >> 4;
        match pci_type {
            0x0 => {
                let len = (first[0] & 0x0F) as usize;
                if len == 0 || len > 7 {
                    return Err(HandosError::Protocol(format!(
                        "invalid ISO-TP single-frame length: {len}"
                    )));
                }
                Ok(first[1..=len].to_vec())
            }
            0x1 => {
                let total_len = (((first[0] & 0x0F) as usize) << 8) | (first[1] as usize);
                if total_len <= 7 || total_len > 4095 {
                    return Err(HandosError::Protocol(format!(
                        "invalid ISO-TP first-frame length: {total_len}"
                    )));
                }
                let mut out = Vec::with_capacity(total_len);
                out.extend_from_slice(&first[2..8]);
                let mut expected_sn: u8 = 1;
                for frame in &frames[1..] {
                    if (frame[0] >> 4) != 0x2 {
                        return Err(HandosError::Protocol(format!(
                            "expected ISO-TP consecutive frame (0x2_), got 0x{:02X}",
                            frame[0]
                        )));
                    }
                    let sn = frame[0] & 0x0F;
                    if sn != expected_sn {
                        return Err(HandosError::Protocol(format!(
                            "ISO-TP sequence mismatch: expected {expected_sn}, got {sn}"
                        )));
                    }
                    let remaining = total_len - out.len();
                    let take = remaining.min(7);
                    out.extend_from_slice(&frame[1..1 + take]);
                    expected_sn = (expected_sn + 1) & 0x0F;
                    if out.len() >= total_len {
                        break;
                    }
                }
                if out.len() != total_len {
                    return Err(HandosError::Protocol(format!(
                        "incomplete ISO-TP message: expected {total_len} bytes, got {}",
                        out.len()
                    )));
                }
                Ok(out)
            }
            other => Err(HandosError::Protocol(format!(
                "unsupported ISO-TP PCI type: 0x{other:X}"
            ))),
        }
    }
}

/// K-Line wake-up and initialization helpers (ISO 14230 Fast Init and ISO 9141/14230 5-Baud Init).
pub struct KlineInit;

impl KlineInit {
    /// Builds a KWP2000 Fast Init `StartCommunication` (`0x81`) frame.
    pub fn fast_init_request(target_ecu: u8) -> Result<Vec<u8>> {
        KwpPacket::new(target_ecu, vec![0x81]).encode()
    }

    /// Validates a `StartCommunication` positive response (`0xC1`) and returns key bytes `(kb1, kb2)`.
    pub fn validate_fast_init_response(raw: &[u8]) -> Result<(u8, u8)> {
        let pkt = KwpPacket::decode(raw)?;
        if pkt.payload.first().copied() != Some(0xC1) || pkt.payload.len() < 3 {
            return Err(HandosError::Protocol(
                "invalid K-Line StartCommunication response".to_string(),
            ));
        }
        Ok((pkt.payload[1], pkt.payload[2]))
    }

    /// Computes the tester acknowledgement byte (`~kw2`) for 5-baud slow initialization.
    pub fn five_baud_ack_byte(kw2: u8) -> u8 {
        !kw2
    }
}
