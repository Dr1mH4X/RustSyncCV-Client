//! LAN protocol message types.
//!
//! All messages exchanged over UDP (discovery) and TCP (peer data) are defined
//! here so that every sub-module speaks the same language.

use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// Constants
// ────────────────────────────────────────────────────────────────────────────

/// Magic bytes prepended to every UDP discovery datagram to avoid collisions
/// with other broadcast traffic on the same port.
pub const DISCOVERY_MAGIC: &[u8; 8] = b"RSCV_LAN";

/// Default UDP port used for broadcast discovery.
pub const DEFAULT_DISCOVERY_PORT: u16 = 52741;

/// Default TCP port the "host" peer listens on for data connections.
pub const DEFAULT_TCP_PORT: u16 = 52742;

/// How often a discovery beacon is broadcast (seconds).
pub const DISCOVERY_INTERVAL_SECS: u64 = 3;

/// Heartbeat ping interval (seconds).
pub const HEARTBEAT_INTERVAL_SECS: u64 = 5;

/// If no pong is received within this many seconds the connection is
/// considered dead.
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 15;

/// Back-off ceiling for reconnection attempts (seconds).
pub const MAX_RECONNECT_DELAY_SECS: u64 = 30;

/// Initial reconnection delay (seconds).
pub const INITIAL_RECONNECT_DELAY_SECS: u64 = 1;

// ────────────────────────────────────────────────────────────────────────────
// UDP Discovery
// ────────────────────────────────────────────────────────────────────────────

/// Broadcast beacon payload — sent periodically over UDP.
///
/// Every node on the LAN that receives this can decide whether to connect.
/// The `device_id` disambiguates peers, and `device_name` is a human-readable
/// label shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryBeacon {
    /// Unique identifier for this peer (UUID v4, generated once per session).
    pub device_id: String,
    /// Human-friendly device name (e.g. hostname).
    pub device_name: String,
    /// TCP port that this peer is listening on for data connections.
    pub tcp_port: u16,
    /// Monotonically increasing sequence number; lets receivers detect
    /// restarts or duplicate beacons.
    pub seq: u64,
}

// ────────────────────────────────────────────────────────────────────────────
// TCP Peer Messages  (length-prefixed JSON over a raw TCP stream)
// ────────────────────────────────────────────────────────────────────────────

/// Top-level envelope for every message exchanged on a TCP peer connection.
///
/// We use an internal tag so serde serialises it as:
/// ```json
/// { "type": "Ping", ... }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PeerMessage {
    // ── Handshake ────────────────────────────────────────────────────────
    /// Sent by the *connecting* side immediately after TCP connect.
    Hello {
        device_id: String,
        device_name: String,
    },
    /// Reply from the *host* side acknowledging the handshake.
    Welcome {
        device_id: String,
        device_name: String,
    },

    // ── Heartbeat ────────────────────────────────────────────────────────
    Ping {
        ts: u64,
    },
    Pong {
        ts: u64,
    },

    // ── Clipboard data ───────────────────────────────────────────────────
    /// Clipboard payload — reuses the same shape as the existing
    /// `ClipboardBroadcastPayload` so the setter logic doesn't change.
    Clipboard {
        content_type: String,
        data: String,
    },
}

// ────────────────────────────────────────────────────────────────────────────
// Discovered peer (stored in shared state)
// ────────────────────────────────────────────────────────────────────────────

/// A peer that has been seen on the LAN via discovery beacons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub device_name: String,
    pub addr: String, // IP address (no port)
    pub tcp_port: u16,
    pub last_seen: u64, // unix timestamp (secs)
}

// ────────────────────────────────────────────────────────────────────────────
// Wire helpers
// ────────────────────────────────────────────────────────────────────────────

/// Encode a [`DiscoveryBeacon`] into a datagram with the magic prefix.
pub fn encode_beacon(beacon: &DiscoveryBeacon) -> Vec<u8> {
    let json = serde_json::to_vec(beacon).expect("beacon serialisation is infallible");
    let mut buf = Vec::with_capacity(DISCOVERY_MAGIC.len() + json.len());
    buf.extend_from_slice(DISCOVERY_MAGIC);
    buf.extend_from_slice(&json);
    buf
}

/// Try to decode a [`DiscoveryBeacon`] from a raw datagram.
/// Returns `None` when the magic prefix doesn't match or the JSON is invalid.
pub fn decode_beacon(data: &[u8]) -> Option<DiscoveryBeacon> {
    if data.len() <= DISCOVERY_MAGIC.len() {
        return None;
    }
    if &data[..DISCOVERY_MAGIC.len()] != DISCOVERY_MAGIC {
        return None;
    }
    serde_json::from_slice(&data[DISCOVERY_MAGIC.len()..]).ok()
}

/// Encode a [`PeerMessage`] into a length-prefixed frame:
///
/// ```text
/// [4 bytes big-endian length][JSON payload]
/// ```
pub fn encode_peer_message(msg: &PeerMessage) -> Vec<u8> {
    let json = serde_json::to_vec(msg).expect("peer message serialisation is infallible");
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&json);
    buf
}

/// Maximum allowed frame size (16 MiB) to avoid unbounded allocations from
/// a misbehaving peer.
pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;
