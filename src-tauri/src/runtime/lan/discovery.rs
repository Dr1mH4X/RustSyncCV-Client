//! UDP broadcast discovery for LAN peers.
//!
//! This module provides two async tasks:
//!
//! 1. **Beacon broadcaster** — periodically sends a [`DiscoveryBeacon`] as a
//!    UDP broadcast so that other peers on the same LAN segment can find us.
//!
//! 2. **Beacon listener** — listens for incoming beacons from other peers and
//!    maintains a shared [`DiscoveredPeers`] map that the rest of the system
//!    can query.
//!
//! Both tasks respect a [`CancellationToken`] for clean shutdown and emit
//! [`RuntimeEvent`]s so the UI can display discovery status.

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use log::Level;
use parking_lot::RwLock;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::{
    net::UdpSocket,
    sync::mpsc,
    time::{interval, Duration},
};
use tokio_util::sync::CancellationToken;

use super::protocol::{
    decode_beacon, encode_beacon, DiscoveredPeer, DiscoveryBeacon, DEFAULT_DISCOVERY_PORT,
    DISCOVERY_INTERVAL_SECS,
};
use crate::runtime::{RuntimeEvent, RuntimeLogEvent};

// ────────────────────────────────────────────────────────────────────────────
// Shared peer map
// ────────────────────────────────────────────────────────────────────────────

/// Thread-safe container for discovered peers, keyed by `device_id`.
///
/// Wrapped in an `Arc<RwLock<…>>` so that the broadcaster, listener, and
/// connection manager can all access it without contention issues.
pub type DiscoveredPeers = Arc<RwLock<HashMap<String, DiscoveredPeer>>>;

/// Create a new, empty peer map.
pub fn new_peer_map() -> DiscoveredPeers {
    Arc::new(RwLock::new(HashMap::new()))
}

/// How many seconds before a peer that has not re-announced is considered
/// stale and removed from the map.
const PEER_EXPIRY_SECS: u64 = 15;

// ────────────────────────────────────────────────────────────────────────────
// Beacon broadcaster
// ────────────────────────────────────────────────────────────────────────────

/// Periodically broadcasts a discovery beacon on the LAN.
///
/// The socket is bound to `0.0.0.0:0` (ephemeral port) with `SO_BROADCAST`
/// enabled, and the beacon is sent to `255.255.255.255:<discovery_port>`.
///
/// # Arguments
///
/// * `device_id`      — unique identifier for this peer (UUID v4).
/// * `device_name`    — human-friendly label (e.g. hostname).
/// * `tcp_port`       — the TCP port we are listening on for peer connections.
/// * `discovery_port` — UDP port to broadcast on (use `0` for the default).
/// * `events`         — channel to emit runtime events for logging / UI.
/// * `cancel`         — token to signal graceful shutdown.
pub async fn run_beacon_broadcaster(
    device_id: String,
    device_name: String,
    tcp_port: u16,
    discovery_port: u16,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let port = if discovery_port == 0 {
        DEFAULT_DISCOVERY_PORT
    } else {
        discovery_port
    };

    // Bind to INADDR_ANY so the OS picks the right interface.
    // We use port 0 for the *sending* socket so we don't conflict with the
    // listener that is bound to the same discovery port.
    let socket = match UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await {
        Ok(s) => s,
        Err(e) => {
            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Error,
                    format!("LAN discovery broadcaster bind failed: {}", e),
                )))
                .await;
            return;
        }
    };

    if let Err(e) = socket.set_broadcast(true) {
        let _ = events
            .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                Level::Error,
                format!("LAN discovery broadcaster set_broadcast failed: {}", e),
            )))
            .await;
        return;
    }

    let broadcast_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, port));

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Info,
            format!(
                "LAN discovery broadcaster started (port={}, tcp_port={})",
                port, tcp_port
            ),
        )))
        .await;

    let mut seq: u64 = 0;
    let mut tick = interval(Duration::from_secs(DISCOVERY_INTERVAL_SECS));

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {
                let beacon = DiscoveryBeacon {
                    device_id: device_id.clone(),
                    device_name: device_name.clone(),
                    tcp_port,
                    seq,
                };
                let packet = encode_beacon(&beacon);
                if let Err(e) = socket.send_to(&packet, broadcast_addr).await {
                    let _ = events
                        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                            Level::Warn,
                            format!("LAN beacon send failed: {}", e),
                        )))
                        .await;
                }
                seq = seq.wrapping_add(1);
            }
        }
    }

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            "LAN discovery broadcaster stopped",
        )))
        .await;
}

// ────────────────────────────────────────────────────────────────────────────
// Beacon listener
// ────────────────────────────────────────────────────────────────────────────

/// Listens for discovery beacons from LAN peers and maintains `peers`.
///
/// Beacons whose `device_id` matches our own are silently ignored (we don't
/// want to discover ourselves). Stale peers that haven't sent a beacon within
/// [`PEER_EXPIRY_SECS`] seconds are pruned on every receive cycle.
///
/// Whenever the peer map changes (a new peer appears, an existing peer's
/// fields are updated, or a stale peer is removed) a
/// [`RuntimeEvent::LanPeersChanged`] event is emitted so the frontend can
/// refresh its peer list.
///
/// # Arguments
///
/// * `own_device_id` — our device id, used to filter self-beacons.
/// * `peers`         — shared map that will be updated in-place.
/// * `socket`        — a pre-bound UDP socket (created via
///                     [`bind_reusable_udp`] by the caller so that bind
///                     failures can be surfaced before any tasks are
///                     spawned).
/// * `events`        — channel to emit runtime events.
/// * `cancel`        — token to signal graceful shutdown.
pub async fn run_beacon_listener(
    own_device_id: String,
    peers: DiscoveredPeers,
    socket: UdpSocket,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Info,
            "LAN discovery listener started",
        )))
        .await;

    let mut buf = [0u8; 2048];

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, src_addr)) => {
                        if let Some(beacon) = decode_beacon(&buf[..len]) {
                            // Ignore our own beacons.
                            if beacon.device_id == own_device_id {
                                continue;
                            }

                            let now = now_unix_secs();
                            let ip = src_addr.ip().to_string();

                            let changed = upsert_peer(&peers, &beacon, &ip, now);

                            if changed {
                                let _ = events
                                    .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                                        Level::Info,
                                        format!(
                                            "LAN peer discovered/updated: {} ({}) at {}:{}",
                                            beacon.device_name,
                                            beacon.device_id,
                                            ip,
                                            beacon.tcp_port,
                                        ),
                                    )))
                                    .await;

                                emit_peer_list(&peers, &events).await;
                            }

                            // Prune stale peers.
                            let pruned = prune_stale_peers(&peers, now);
                            if pruned > 0 {
                                emit_peer_list(&peers, &events).await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = events
                            .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                                Level::Warn,
                                format!("LAN discovery recv error: {}", e),
                            )))
                            .await;
                    }
                }
            }
        }
    }

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            "LAN discovery listener stopped",
        )))
        .await;
}

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

/// Bind a UDP socket with `SO_REUSEADDR` (and `SO_REUSEPORT` where
/// available) using the [`socket2`] crate so that multiple processes on the
/// same machine can share the discovery port during development.
///
/// This is cross-platform — it works on Windows, macOS, and Linux without
/// any raw `libc` or `unsafe` code.
///
/// The function is `pub` within the crate so that the parent module can
/// pre-bind the socket and pass it to [`run_beacon_listener`], allowing
/// bind failures to be surfaced before any background tasks are spawned.
pub async fn bind_reusable_udp(
    port: u16,
    events: &mpsc::Sender<RuntimeEvent>,
) -> Option<UdpSocket> {
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);

    // Create a socket2 socket so we can set options *before* binding.
    let socket = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
        Ok(s) => s,
        Err(e) => {
            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Error,
                    format!("LAN discovery listener: failed to create socket: {}", e),
                )))
                .await;
            return None;
        }
    };

    // SO_REUSEADDR — allow binding even if the port is in TIME_WAIT, and
    // on some platforms allow multiple sockets on the same port.
    if let Err(e) = socket.set_reuse_address(true) {
        let _ = events
            .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                Level::Warn,
                format!(
                    "LAN discovery listener: SO_REUSEADDR failed (non-fatal): {}",
                    e
                ),
            )))
            .await;
    }

    // SO_REUSEPORT — available on macOS / Linux; silently skip on Windows
    // where it doesn't exist.
    #[cfg(not(target_os = "windows"))]
    {
        if let Err(e) = socket.set_reuse_port(true) {
            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Warn,
                    format!(
                        "LAN discovery listener: SO_REUSEPORT failed (non-fatal): {}",
                        e
                    ),
                )))
                .await;
        }
    }

    // Set non-blocking *before* converting to a tokio socket.
    socket.set_nonblocking(true).ok();

    if let Err(e) = socket.bind(&socket2::SockAddr::from(addr)) {
        let _ = events
            .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                Level::Error,
                format!(
                    "LAN discovery listener: bind failed on port {}: {}",
                    port, e
                ),
            )))
            .await;
        return None;
    }

    // Convert socket2 → std → tokio.
    let std_socket: std::net::UdpSocket = socket.into();
    match UdpSocket::from_std(std_socket) {
        Ok(s) => Some(s),
        Err(e) => {
            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Error,
                    format!("LAN discovery listener: tokio conversion failed: {}", e),
                )))
                .await;
            None
        }
    }
}

/// Insert or update a peer entry. Returns `true` when the peer map was
/// meaningfully changed (new peer, or an existing peer's `device_name`,
/// `addr`, or `tcp_port` differs from the stored entry).
fn upsert_peer(peers: &DiscoveredPeers, beacon: &DiscoveryBeacon, ip: &str, now: u64) -> bool {
    let mut writer = peers.write();

    let new_entry = DiscoveredPeer {
        device_id: beacon.device_id.clone(),
        device_name: beacon.device_name.clone(),
        addr: ip.to_string(),
        tcp_port: beacon.tcp_port,
        last_seen: now,
    };

    if let Some(existing) = writer.get(&beacon.device_id) {
        let changed = existing.device_name != new_entry.device_name
            || existing.addr != new_entry.addr
            || existing.tcp_port != new_entry.tcp_port;

        // Always update `last_seen`, but only report a change when
        // user-visible fields actually differ.
        writer.insert(beacon.device_id.clone(), new_entry);
        changed
    } else {
        writer.insert(beacon.device_id.clone(), new_entry);
        true // brand-new peer
    }
}

/// Removes peers that haven't been seen within [`PEER_EXPIRY_SECS`] and
/// returns the number of entries removed.
fn prune_stale_peers(peers: &DiscoveredPeers, now: u64) -> usize {
    let mut writer = peers.write();
    let before = writer.len();
    writer.retain(|_, peer| now.saturating_sub(peer.last_seen) < PEER_EXPIRY_SECS);
    before - writer.len()
}

/// Emit the current peer list as a JSON-serialised
/// [`RuntimeEvent::LanPeersChanged`] so the frontend can refresh the
/// displayed peer list.
async fn emit_peer_list(peers: &DiscoveredPeers, events: &mpsc::Sender<RuntimeEvent>) {
    let list: Vec<DiscoveredPeer> = peers.read().values().cloned().collect();
    let json = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
    let _ = events.send(RuntimeEvent::LanPeersChanged(json)).await;
}

/// Returns the current UNIX timestamp in seconds.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Return a snapshot of all currently discovered peers.
pub fn get_discovered_peers(peers: &DiscoveredPeers) -> Vec<DiscoveredPeer> {
    peers.read().values().cloned().collect()
}
