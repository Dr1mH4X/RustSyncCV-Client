//! LAN mode orchestrator.
//!
//! This module ties together discovery, peer connection, and the existing
//! clipboard monitor/setter infrastructure to provide a fully serverless
//! clipboard-sync experience on a local network.
//!
//! ## Architecture
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────────────────┐
//!  │                        run_lan_mode                            │
//!  │                                                                │
//!  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐ │
//!  │  │  UDP beacon  │  │ UDP listener │  │   TCP host listener  │ │
//!  │  │ broadcaster  │  │  (discovery) │  │  (accepts incoming)  │ │
//!  │  └──────────────┘  └──────┬───────┘  └──────────────────────┘ │
//!  │                           │                                    │
//!  │                    DiscoveredPeers                              │
//!  │                           │                                    │
//!  │                  ┌────────▼─────────┐                          │
//!  │                  │  peer_connector  │  (connects to new peers) │
//!  │                  └──────────────────┘                          │
//!  │                                                                │
//!  │  ┌──────────────────┐          ┌──────────────────┐           │
//!  │  │ clipboard_monitor│──tx_out──│  peer sessions   │           │
//!  │  └──────────────────┘          │  (host + client) │           │
//!  │                                │                  │           │
//!  │  ┌──────────────────┐          │                  │           │
//!  │  │ clipboard_setter │◄──tx_in──┘                  │           │
//!  │  └──────────────────┘                             │           │
//!  └───────────────────────────────────────────────────────────────┘
//! ```
//!
//! The **server-decided** connection model works as follows: every peer
//! runs both a TCP host (listener) and a discovery broadcaster. When peer
//! A discovers peer B, A only initiates a TCP connection to B if
//! `A.device_id > B.device_id` (lexicographic comparison). This ensures
//! exactly one TCP session is established between any two peers without
//! any additional negotiation protocol.

pub mod discovery;
pub mod peer;
pub mod protocol;

use std::{
    collections::HashSet,
    sync::{atomic::AtomicBool, Arc},
};

use log::Level;
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
    time::{sleep, Duration},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use discovery::{get_discovered_peers, new_peer_map, run_beacon_broadcaster, run_beacon_listener};
use peer::{run_tcp_client, run_tcp_host};
use protocol::{DiscoveredPeer, DEFAULT_DISCOVERY_PORT, DEFAULT_TCP_PORT};

use super::clipboard::{start_clipboard_monitor, start_clipboard_setter};
use super::config::Config;
use super::messages::{ClipboardBroadcastPayload, ClipboardUpdate};
use super::{RuntimeEvent, RuntimeLogEvent};

// ────────────────────────────────────────────────────────────────────────────
// LAN-specific configuration defaults
// ────────────────────────────────────────────────────────────────────────────

/// How often the peer-connector scans the discovered-peers map for new
/// peers that we should connect to (seconds).
const CONNECTOR_SCAN_INTERVAL_SECS: u64 = 4;

// ────────────────────────────────────────────────────────────────────────────
// Public entry point
// ────────────────────────────────────────────────────────────────────────────

/// Orchestrates all LAN-mode tasks and returns a set of [`JoinHandle`]s
/// that the caller can await or abort for clean shutdown.
///
/// This is the single public entry point that [`RuntimeWorker`] calls when
/// the user has chosen LAN mode. It:
///
/// 1. Generates a session `device_id` and resolves the local hostname as
///    `device_name`.
/// 2. Spawns the clipboard monitor and setter (reusing the existing
///    implementations).
/// 3. Spawns the UDP beacon broadcaster and listener.
/// 4. Spawns the TCP host listener.
/// 5. Spawns a **peer connector** task that periodically checks the
///    discovered-peers map and opens TCP client connections to any new
///    peer where our `device_id` is lexicographically greater (the
///    "server-decided" rule).
///
/// All tasks share a single [`CancellationToken`]; cancelling it will
/// gracefully stop everything.
pub struct LanTasks {
    pub cancel: CancellationToken,
    pub handles: Vec<JoinHandle<()>>,
}

impl LanTasks {
    /// Cancel all tasks and await their completion.
    pub async fn shutdown(self) {
        self.cancel.cancel();
        for h in self.handles {
            let _ = h.await;
        }
    }

    /// Cancel all tasks and abort them without waiting.
    pub fn abort(self) {
        self.cancel.cancel();
        for h in self.handles {
            h.abort();
        }
    }
}

/// Start all LAN mode tasks.
///
/// Ports are always the built-in defaults ([`DEFAULT_DISCOVERY_PORT`] and
/// [`DEFAULT_TCP_PORT`]) so that all peers on the same LAN segment agree
/// without any user configuration.
///
/// # Arguments
///
/// * `config`              — application configuration (only `max_image_kb`
///                            and `lan_device_name` are read here).
/// * `device_name_override` — optional human-friendly name; if `None` or
///                            empty the system hostname is used.
/// * `events`              — runtime event channel shared with the main
///                            `RuntimeWorker`.
/// * `cancel`              — parent cancellation token; we derive child
///                            tokens so the caller can stop everything.
pub fn start_lan_mode(
    config: &Config,
    device_name_override: Option<String>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) -> LanTasks {
    let device_id = Uuid::new_v4().to_string();
    let device_name = device_name_override
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            hostname::get()
                .ok()
                .and_then(|h| h.into_string().ok())
                .unwrap_or_else(|| format!("RustSyncCV-{}", &device_id[..8]))
        });

    let discovery_port = DEFAULT_DISCOVERY_PORT;
    let tcp_port = DEFAULT_TCP_PORT;

    // Shared channels — same pattern as the existing WebSocket runtime.
    let disable_flag = Arc::new(AtomicBool::new(false));
    let (tx_out, _) = broadcast::channel::<ClipboardUpdate>(100);
    let (tx_in, rx_in) = mpsc::channel::<ClipboardBroadcastPayload>(100);

    // Discovered peers map — shared between listener and connector.
    let peers = new_peer_map();

    let mut handles: Vec<JoinHandle<()>> = Vec::new();

    // ── 1. Clipboard monitor ─────────────────────────────────────────────
    {
        let ev = events.clone();
        let ct = cancel.clone();
        let df = disable_flag.clone();
        let did = device_id.clone();
        let max_kb = config.max_image_kb;
        let tx = tx_out.clone();
        handles.push(tokio::spawn(async move {
            start_clipboard_monitor(tx, df, did, max_kb, ev, ct).await;
        }));
    }

    // ── 2. Clipboard setter ──────────────────────────────────────────────
    {
        let ev = events.clone();
        let ct = cancel.clone();
        let df = disable_flag.clone();
        handles.push(tokio::spawn(async move {
            start_clipboard_setter(rx_in, df, ev, ct).await;
        }));
    }

    // ── 3. UDP beacon broadcaster ────────────────────────────────────────
    {
        let did = device_id.clone();
        let dname = device_name.clone();
        let ev = events.clone();
        let ct = cancel.clone();
        handles.push(tokio::spawn(async move {
            run_beacon_broadcaster(did, dname, tcp_port, discovery_port, ev, ct).await;
        }));
    }

    // ── 4. UDP beacon listener ───────────────────────────────────────────
    {
        let did = device_id.clone();
        let pm = peers.clone();
        let ev = events.clone();
        let ct = cancel.clone();
        handles.push(tokio::spawn(async move {
            run_beacon_listener(did, discovery_port, pm, ev, ct).await;
        }));
    }

    // ── 5. TCP host listener ─────────────────────────────────────────────
    {
        let did = device_id.clone();
        let dname = device_name.clone();
        let tx = tx_out.clone();
        let ti = tx_in.clone();
        let ev = events.clone();
        let ct = cancel.clone();
        handles.push(tokio::spawn(async move {
            run_tcp_host(did, dname, tcp_port, tx, ti, ev, ct).await;
        }));
    }

    // ── 6. Peer connector (server-decided: higher device_id connects) ───
    {
        let own_id = device_id.clone();
        let own_name = device_name.clone();
        let pm = peers.clone();
        let tx = tx_out.clone();
        let ti = tx_in.clone();
        let ev = events.clone();
        let ct = cancel.clone();
        handles.push(tokio::spawn(async move {
            run_peer_connector(own_id, own_name, pm, tx, ti, ev, ct).await;
        }));
    }

    // Emit a startup message.
    {
        let ev = events.clone();
        let did = device_id.clone();
        let dname = device_name.clone();
        tokio::spawn(async move {
            let _ = ev
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Info,
                    format!(
                        "LAN mode started — id={}, name={}, discovery_port={}, tcp_port={}",
                        did, dname, discovery_port, tcp_port,
                    ),
                )))
                .await;
            let _ = ev
                .send(RuntimeEvent::Status(format!("LAN mode active ({})", dname)))
                .await;
        });
    }

    LanTasks { cancel, handles }
}

// ────────────────────────────────────────────────────────────────────────────
// Peer connector task
// ────────────────────────────────────────────────────────────────────────────

/// Periodically scans the discovered-peers map and initiates TCP connections
/// to peers that we should connect to according to the **server-decided**
/// rule: we only connect to peer B if `our_device_id > B.device_id`
/// (lexicographic). The peer with the smaller id acts as the passive
/// acceptor (host).
///
/// Once a connection is initiated to a peer, the peer's `device_id` is added
/// to a local `connected` set so we don't spawn duplicate client tasks. If
/// the client task ends (disconnect / error) it will internally loop and
/// retry with exponential back-off; we don't need to re-spawn it here.
async fn run_peer_connector(
    own_device_id: String,
    own_device_name: String,
    peers: discovery::DiscoveredPeers,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let mut connected: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = sleep(Duration::from_secs(CONNECTOR_SCAN_INTERVAL_SECS)) => {}
        }

        if cancel.is_cancelled() {
            break;
        }

        let current_peers: Vec<DiscoveredPeer> = get_discovered_peers(&peers);

        for peer in current_peers {
            // Server-decided rule: only the side with the *greater* id
            // initiates the connection.
            if own_device_id <= peer.device_id {
                continue;
            }

            if connected.contains(&peer.device_id) {
                continue;
            }

            let addr = format!("{}:{}", peer.addr, peer.tcp_port);

            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Info,
                    format!(
                        "LAN connector: initiating connection to {} ({}) at {}",
                        peer.device_name, peer.device_id, addr,
                    ),
                )))
                .await;

            connected.insert(peer.device_id.clone());

            let did = own_device_id.clone();
            let dname = own_device_name.clone();
            let tx = tx_out.clone();
            let ti = tx_in.clone();
            let ev = events.clone();
            let ct = cancel.child_token();
            let peer_id = peer.device_id.clone();

            // Spawn a long-lived client task that will internally handle
            // reconnection.  We don't track the JoinHandle here; the
            // CancellationToken will stop it on shutdown.
            tokio::spawn(async move {
                run_tcp_client(addr, did, dname, tx, ti, ev.clone(), ct).await;
                let _ = ev
                    .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                        Level::Debug,
                        format!("LAN client task for peer {} exited", peer_id),
                    )))
                    .await;
            });
        }
    }

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            "LAN peer connector stopped",
        )))
        .await;
}
