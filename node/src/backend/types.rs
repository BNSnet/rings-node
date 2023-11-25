#![warn(missing_docs)]

//! Backend Message Types.

use std::io::ErrorKind as IOErrorKind;

use bytes::Bytes;
use rings_core::message::MessagePayload;
use serde::Deserialize;
use serde::Serialize;

use crate::error::Result;

/// TunnelId type, use uuid.
pub type TunnelId = uuid::Uuid;

/// BackendMessage struct for handling CustomMessage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BackendMessage {
    /// extension message
    Extension(Bytes),
    /// server message
    ServerMessage(ServerMessage),
    /// Plain text
    PlainText(String),
}

/// ServerMessage
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ServerMessage {
    /// Tunnel Open
    TcpDial {
        /// Tunnel Id
        tid: TunnelId,
        /// service name
        service: String,
    },
    /// Tunnel Close
    TcpClose {
        /// Tunnel Id
        tid: TunnelId,
        /// The reason of close
        reason: TunnelDefeat,
    },
    /// Send Tcp Package
    TcpPackage {
        /// Tunnel Id
        tid: TunnelId,
        /// Tcp Package
        body: Bytes,
    },
    /// Http Request
    HttpRequest(HttpRequest),
    /// Http Response
    HttpResponse(HttpResponse),
}

/// A list specifying general categories of Tunnel error like [std::io::ErrorKind].
#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
#[repr(u8)]
#[non_exhaustive]
pub enum TunnelDefeat {
    /// Failed to send data to peer by webrtc datachannel.
    WebrtcDatachannelSendFailed = 1,
    /// The connection timed out when dialing.
    ConnectionTimeout = 2,
    /// Got [std::io::ErrorKind::ConnectionRefused] error from local stream.
    ConnectionRefused = 3,
    /// Got [std::io::ErrorKind::ConnectionAborted] error from local stream.
    ConnectionAborted = 4,
    /// Got [std::io::ErrorKind::ConnectionReset] error from local stream.
    ConnectionReset = 5,
    /// Got [std::io::ErrorKind::NotConnected] error from local stream.
    NotConnected = 6,
    /// The connection is closed by peer.
    ConnectionClosed = 7,
    /// Unknown [std::io::ErrorKind] error.
    Unknown = u8::MAX,
}

/// HttpRequest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRequest {
    /// Service name
    pub service: String,
    /// Method
    pub method: String,
    /// Path
    pub path: String,
    /// Headers
    pub headers: Vec<(String, String)>,
    /// Body
    pub body: Option<Vec<u8>>,
}

/// HttpResponse
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpResponse {
    /// Status
    pub status: u16,
    /// Headers
    pub headers: Vec<(String, String)>,
    /// Body
    pub body: Option<Bytes>,
}

/// IntoBackendMessage trait
pub trait IntoBackendMessage {
    /// into_backend_message
    fn into_backend_message(self) -> BackendMessage;
}

/// MessageEndpoint trait
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait MessageEndpoint<T> {
    /// handle_message
    async fn handle_message(&self, ctx: &MessagePayload, data: &T) -> Result<()>;
}

impl IntoBackendMessage for BackendMessage {
    fn into_backend_message(self) -> BackendMessage {
        self
    }
}

impl IntoBackendMessage for ServerMessage {
    fn into_backend_message(self) -> BackendMessage {
        BackendMessage::ServerMessage(self)
    }
}

impl From<IOErrorKind> for TunnelDefeat {
    fn from(kind: IOErrorKind) -> TunnelDefeat {
        match kind {
            IOErrorKind::ConnectionRefused => TunnelDefeat::ConnectionRefused,
            IOErrorKind::ConnectionAborted => TunnelDefeat::ConnectionAborted,
            IOErrorKind::ConnectionReset => TunnelDefeat::ConnectionReset,
            IOErrorKind::NotConnected => TunnelDefeat::NotConnected,
            _ => TunnelDefeat::Unknown,
        }
    }
}
