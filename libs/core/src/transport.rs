use crate::error::{HandosError, Result};
use crate::protocol::{DiagnosticFrame, DEFAULT_MHD_WIFI_HOST, DEFAULT_MHD_WIFI_PORT};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Supported BMW OBD adapter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterType {
    MhdWifi,
    KdcanUsb,
}

/// Transport protocol mode for `HandosSession`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportMode {
    Simulator,
    LiveKwpDcan,
    LiveEnet,
}

/// Probes candidate MHD Universal WiFi adapter endpoints in parallel and returns all reachable ones.
pub fn discover_mhd_endpoints(timeout: Duration, extra_candidates: &[String]) -> Vec<String> {
    let mut candidates = Vec::new();
    for c in extra_candidates {
        let trimmed = c.trim();
        if !trimmed.is_empty() && !candidates.iter().any(|existing| existing == trimmed) {
            candidates.push(trimmed.to_string());
        }
    }
    for default_addr in [
        format!("{DEFAULT_MHD_WIFI_HOST}:{DEFAULT_MHD_WIFI_PORT}"),
        format!("192.168.1.1:{DEFAULT_MHD_WIFI_PORT}"),
    ] {
        if !candidates.iter().any(|existing| existing == &default_addr) {
            candidates.push(default_addr);
        }
    }

    std::thread::scope(|s| {
        let handles: Vec<_> = candidates
            .into_iter()
            .map(|candidate| {
                s.spawn(move || {
                    if MhdWifiTransport::connect(&candidate, timeout).is_ok() {
                        Some(candidate)
                    } else {
                        None
                    }
                })
            })
            .collect();

        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    })
}

/// Probes common MHD Universal WiFi adapter endpoints and returns the first reachable one.
pub fn discover_mhd_endpoint(timeout: Duration) -> Option<String> {
    discover_mhd_endpoints(timeout, &[]).into_iter().next()
}

/// Serial baud/bus mode for standard FTDI K+DCAN USB cables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KdcanProtocolMode {
    /// D-CAN (500 kbps CAN via 115200 baud FTDI) for 2007+ E8x/E9x/E60/E70.
    Dcan115200,
    /// K-Line (10400 baud) for E46 / early E39 / early E90 modules.
    Kline10400,
}

impl KdcanProtocolMode {
    pub fn baud_rate(self) -> u32 {
        match self {
            Self::Dcan115200 => 115_200,
            Self::Kline10400 => 10_400,
        }
    }
}

/// Abstraction over the physical/network link to the BMW vehicle (MHD WiFi, K+DCAN USB, simulator).
pub trait Transport: Send {
    fn exchange(&mut self, request: &DiagnosticFrame) -> Result<DiagnosticFrame>;
    fn endpoint(&self) -> &str;
}

impl Transport for Box<dyn Transport> {
    fn exchange(&mut self, request: &DiagnosticFrame) -> Result<DiagnosticFrame> {
        (**self).exchange(request)
    }

    fn endpoint(&self) -> &str {
        (**self).endpoint()
    }
}

/// Non-owning stream wrapper around a Unix file descriptor (e.g. Android `UsbDeviceConnection.getFileDescriptor()`).
/// Dropping this wrapper does NOT close the underlying descriptor, since Android's `UsbDeviceConnection` owns it.
#[cfg(unix)]
pub struct BorrowedFdStream {
    file: std::mem::ManuallyDrop<std::fs::File>,
}

#[cfg(unix)]
impl BorrowedFdStream {
    /// # Safety
    /// `fd` must be a valid, open file descriptor that outlives this `BorrowedFdStream`.
    pub unsafe fn from_raw_fd(fd: std::os::unix::io::RawFd) -> Self {
        use std::os::unix::io::FromRawFd;
        Self {
            file: std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(fd)),
        }
    }
}

#[cfg(unix)]
impl Read for BorrowedFdStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&*self.file).read(buf)
    }
}

#[cfg(unix)]
impl Write for BorrowedFdStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        (&*self.file).write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        (&*self.file).flush()
    }
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

/// Transport for a standard FTDI K+DCAN USB cable (via USB-C OTG on Android or serial port).
pub struct KdcanSerialTransport<S: Read + Write + Send> {
    stream: S,
    mode: KdcanProtocolMode,
    label: String,
}

impl<S: Read + Write + Send> KdcanSerialTransport<S> {
    pub fn new(stream: S, mode: KdcanProtocolMode, label: impl Into<String>) -> Self {
        Self {
            stream,
            mode,
            label: label.into(),
        }
    }

    pub fn mode(&self) -> KdcanProtocolMode {
        self.mode
    }
}

impl<S: Read + Write + Send> Transport for KdcanSerialTransport<S> {
    fn exchange(&mut self, request: &DiagnosticFrame) -> Result<DiagnosticFrame> {
        request.write_to(&mut self.stream)?;
        DiagnosticFrame::read_from(&mut self.stream)
    }

    fn endpoint(&self) -> &str {
        &self.label
    }
}
