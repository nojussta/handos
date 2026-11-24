use crate::error::{HandosError, Result};
use crate::protocol::{DiagnosticFrame, DEFAULT_MHD_WIFI_HOST, DEFAULT_MHD_WIFI_PORT};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Abstraction over the physical/network link to the BMW vehicle (MHD WiFi, simulator, etc.).
pub trait Transport: Send {
    fn exchange(&mut self, request: &DiagnosticFrame) -> Result<DiagnosticFrame>;
    fn endpoint(&self) -> &str;
}

/// TCP socket transport for the MHD Universal WiFi adapter (default `192.168.4.1:6801`).
pub struct MhdWifiTransport {
    stream: TcpStream,
    endpoint: String,
}

impl MhdWifiTransport {
    pub fn connect_default() -> Result<Self> {
        let addr = format!("{DEFAULT_MHD_WIFI_HOST}:{DEFAULT_MHD_WIFI_PORT}");
        Self::connect(&addr, Duration::from_secs(5))
    }

    pub fn connect(address: &str, timeout: Duration) -> Result<Self> {
        let sock_addr: SocketAddr = address
            .to_socket_addrs()
            .map_err(|e| HandosError::Connection {
                address: address.to_string(),
                reason: e.to_string(),
            })?
            .next()
            .ok_or_else(|| HandosError::Connection {
                address: address.to_string(),
                reason: "could not resolve address".to_string(),
            })?;

        let stream = TcpStream::connect_timeout(&sock_addr, timeout).map_err(|e| {
            HandosError::Connection {
                address: address.to_string(),
                reason: e.to_string(),
            }
        })?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;

        Ok(Self {
            stream,
            endpoint: address.to_string(),
        })
    }
}

impl Transport for MhdWifiTransport {
    fn exchange(&mut self, request: &DiagnosticFrame) -> Result<DiagnosticFrame> {
        request.write_to(&mut self.stream)?;
        DiagnosticFrame::read_from(&mut self.stream)
    }

    fn endpoint(&self) -> &str {
        &self.endpoint
    }
}
