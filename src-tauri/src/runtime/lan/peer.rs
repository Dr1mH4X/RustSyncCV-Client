//! TCP peer connection module — host/client roles, heartbeat, reconnection,
//! clipboard relay.
//!
//! This module provides two entry-point tasks:
//!
//! 1. **`run_tcp_host`** — binds a TCP listener and accepts incoming peer
//!    connections. For each accepted connection it spawns a session task that
//!    handles the handshake, heartbeat, and bidirectional clipboard relay.
//!
//! 2. **`run_tcp_client`** — connects to a remote peer's TCP listener, performs
//!    the handshake, and then enters the same session loop. If the connection
//!    drops it will attempt to reconnect with exponential back-off up to
//!    [`MAX_RECONNECT_DELAY_SECS`].
//!
//! Both tasks share the same [`run_peer_session`] function for the steady-state
//! loop so that heartbeat, clipboard send/receive, and error handling are
//! written exactly once (DRY).

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use log::Level;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
    time::{interval, sleep, Duration, Instant},
};
use tokio_util::sync::CancellationToken;

use super::protocol::{
    encode_peer_message, PeerMessage, DEFAULT_TCP_PORT, HEARTBEAT_INTERVAL_SECS,
    HEARTBEAT_TIMEOUT_SECS, INITIAL_RECONNECT_DELAY_SECS, MAX_FRAME_SIZE, MAX_RECONNECT_DELAY_SECS,
};
use crate::runtime::messages::{
    ClipboardBroadcastPayload, ClipboardUpdate, CONTENT_TYPE_IMAGE_PNG, CONTENT_TYPE_TEXT,
};
use crate::runtime::{RuntimeEvent, RuntimeLogEvent};

// ────────────────────────────────────────────────────────────────────────────
// Public API — Host
// ────────────────────────────────────────────────────────────────────────────

/// Bind a TCP listener and accept peer connections.
///
/// Each accepted connection is handled in its own spawned task via
/// [`run_peer_session`]. The host sends a `Welcome` after receiving the
/// client's `Hello`.
///
/// # Arguments
///
/// * `device_id`   — our unique peer identifier.
/// * `device_name` — human-friendly device name (displayed on the remote).
/// * `tcp_port`    — port to listen on (`0` → [`DEFAULT_TCP_PORT`]).
/// * `tx_out`      — broadcast channel carrying outbound clipboard updates
///                    (from our local clipboard monitor).
/// * `tx_in`       — channel to forward inbound clipboard payloads into the
///                    local clipboard setter.
/// * `events`      — runtime event sink for logging / UI.
/// * `cancel`      — token to signal graceful shutdown.
pub async fn run_tcp_host(
    device_id: String,
    device_name: String,
    tcp_port: u16,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let port = if tcp_port == 0 {
        DEFAULT_TCP_PORT
    } else {
        tcp_port
    };

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            emit_log(
                &events,
                Level::Error,
                format!("LAN host TCP bind failed on {}: {}", bind_addr, e),
            )
            .await;
            return;
        }
    };

    emit_log(
        &events,
        Level::Info,
        format!("LAN host listening on {}", bind_addr),
    )
    .await;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, peer_addr)) => {
                        emit_log(
                            &events,
                            Level::Info,
                            format!("LAN host accepted connection from {}", peer_addr),
                        ).await;

                        let did = device_id.clone();
                        let dname = device_name.clone();
                        let tx_o = tx_out.clone();
                        let tx_i = tx_in.clone();
                        let ev = events.clone();
                        let ct = cancel.child_token();

                        tokio::spawn(async move {
                            if let Err(e) = host_session(
                                stream, did, dname, tx_o, tx_i, ev.clone(), ct,
                            ).await {
                                emit_log(
                                    &ev,
                                    Level::Warn,
                                    format!("LAN host session with {} ended: {}", peer_addr, e),
                                ).await;
                            }
                        });
                    }
                    Err(e) => {
                        emit_log(
                            &events,
                            Level::Warn,
                            format!("LAN host accept error: {}", e),
                        ).await;
                    }
                }
            }
        }
    }

    emit_log(&events, Level::Debug, "LAN host listener stopped").await;
}

// ────────────────────────────────────────────────────────────────────────────
// Public API — Client (with reconnection)
// ────────────────────────────────────────────────────────────────────────────

/// Connect to a remote peer and relay clipboard data, reconnecting on failure.
///
/// The function enters an outer loop that keeps attempting to establish a TCP
/// connection. Once connected it performs the `Hello`/`Welcome` handshake and
/// delegates to [`run_peer_session`]. When the session ends (error or remote
/// disconnect) it waits with exponential back-off before retrying.
///
/// # Arguments
///
/// * `peer_addr`   — `"<ip>:<port>"` of the remote host.
/// * `device_id`   — our unique peer identifier.
/// * `device_name` — our human-friendly device name.
/// * `tx_out`      — broadcast channel carrying outbound clipboard updates.
/// * `tx_in`       — channel to forward inbound clipboard payloads.
/// * `events`      — runtime event sink.
/// * `cancel`      — token to signal graceful shutdown.
pub async fn run_tcp_client(
    peer_addr: String,
    device_id: String,
    device_name: String,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let mut delay_secs = INITIAL_RECONNECT_DELAY_SECS;

    while !cancel.is_cancelled() {
        emit_log(
            &events,
            Level::Info,
            format!("LAN client connecting to {} …", peer_addr),
        )
        .await;

        let connect_result = tokio::select! {
            _ = cancel.cancelled() => break,
            r = TcpStream::connect(&peer_addr) => r,
        };

        match connect_result {
            Ok(stream) => {
                emit_log(
                    &events,
                    Level::Info,
                    format!("LAN client connected to {}", peer_addr),
                )
                .await;

                // Reset back-off on successful connect.
                delay_secs = INITIAL_RECONNECT_DELAY_SECS;

                let result = client_session(
                    stream,
                    device_id.clone(),
                    device_name.clone(),
                    tx_out.clone(),
                    tx_in.clone(),
                    events.clone(),
                    cancel.child_token(),
                )
                .await;

                if let Err(e) = result {
                    emit_log(
                        &events,
                        Level::Warn,
                        format!("LAN client session with {} ended: {}", peer_addr, e),
                    )
                    .await;
                }
            }
            Err(e) => {
                emit_log(
                    &events,
                    Level::Warn,
                    format!("LAN client connect to {} failed: {}", peer_addr, e),
                )
                .await;
            }
        }

        if cancel.is_cancelled() {
            break;
        }

        emit_log(
            &events,
            Level::Info,
            format!("LAN client reconnecting in {}s …", delay_secs),
        )
        .await;

        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = sleep(Duration::from_secs(delay_secs)) => {},
        }

        // Exponential back-off with ceiling.
        delay_secs = (delay_secs * 2).min(MAX_RECONNECT_DELAY_SECS);
    }

    emit_log(&events, Level::Debug, "LAN client reconnect loop stopped").await;
}

// ────────────────────────────────────────────────────────────────────────────
// Session entry-points (host / client handshake wrappers)
// ────────────────────────────────────────────────────────────────────────────

/// Host-side session: wait for `Hello`, reply with `Welcome`, then enter
/// the shared session loop.
async fn host_session(
    mut stream: TcpStream,
    device_id: String,
    device_name: String,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    // ── Wait for Hello ───────────────────────────────────────────────────
    let hello = read_peer_message(&mut stream)
        .await
        .context("reading Hello from client")?;

    let (remote_id, remote_name) = match hello {
        PeerMessage::Hello {
            device_id: rid,
            device_name: rname,
        } => (rid, rname),
        other => {
            return Err(anyhow!(
                "expected Hello from client, got {:?}",
                msg_type_name(&other)
            ));
        }
    };

    emit_log(
        &events,
        Level::Info,
        format!(
            "LAN host handshake: remote peer {} ({})",
            remote_name, remote_id
        ),
    )
    .await;

    // ── Send Welcome ─────────────────────────────────────────────────────
    let welcome = PeerMessage::Welcome {
        device_id: device_id.clone(),
        device_name: device_name.clone(),
    };
    write_peer_message(&mut stream, &welcome).await?;

    // ── Enter shared session loop ────────────────────────────────────────
    run_peer_session(stream, tx_out, tx_in, events, cancel).await
}

/// Client-side session: send `Hello`, wait for `Welcome`, then enter the
/// shared session loop.
async fn client_session(
    mut stream: TcpStream,
    device_id: String,
    device_name: String,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    // ── Send Hello ───────────────────────────────────────────────────────
    let hello = PeerMessage::Hello {
        device_id: device_id.clone(),
        device_name: device_name.clone(),
    };
    write_peer_message(&mut stream, &hello).await?;

    // ── Wait for Welcome ─────────────────────────────────────────────────
    let welcome = read_peer_message(&mut stream)
        .await
        .context("reading Welcome from host")?;

    let (remote_id, remote_name) = match welcome {
        PeerMessage::Welcome {
            device_id: rid,
            device_name: rname,
        } => (rid, rname),
        other => {
            return Err(anyhow!(
                "expected Welcome from host, got {:?}",
                msg_type_name(&other)
            ));
        }
    };

    emit_log(
        &events,
        Level::Info,
        format!(
            "LAN client handshake OK: remote peer {} ({})",
            remote_name, remote_id
        ),
    )
    .await;

    // ── Enter shared session loop ────────────────────────────────────────
    run_peer_session(stream, tx_out, tx_in, events, cancel).await
}

// ────────────────────────────────────────────────────────────────────────────
// Shared session loop (DRY — used by both host and client)
// ────────────────────────────────────────────────────────────────────────────

/// Bidirectional clipboard relay with heartbeat keep-alive.
///
/// This function is role-agnostic — it works identically whether we are the
/// TCP host or the TCP client. It runs three concurrent concerns via
/// `tokio::select!`:
///
/// 1. **Heartbeat tick** — sends a `Ping` every [`HEARTBEAT_INTERVAL_SECS`]
///    seconds. If the last `Pong` was received longer ago than
///    [`HEARTBEAT_TIMEOUT_SECS`] the connection is considered dead.
///
/// 2. **Outbound clipboard** — listens on `tx_out` for local clipboard
///    changes and forwards them as `PeerMessage::Clipboard`.
///
/// 3. **Inbound read** — reads frames from the TCP stream and dispatches:
///    - `Ping`      → reply with `Pong`
///    - `Pong`      → update last-pong timestamp
///    - `Clipboard` → forward payload into `tx_in`
///    - anything else → log and ignore
async fn run_peer_session(
    stream: TcpStream,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    let (reader_half, writer_half) = stream.into_split();

    // Wrap the writer in a mutex so both heartbeat and clipboard-send can
    // use it without requiring `split` ownership gymnastics.
    let writer = std::sync::Arc::new(tokio::sync::Mutex::new(writer_half));

    let mut rx_updates = tx_out.subscribe();
    let mut heartbeat_tick = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    let mut last_pong = Instant::now();

    // We need a mutable reference to `reader_half` across iterations, so
    // we wrap it in an Option to allow taking ownership in the select loop.
    let reader = std::sync::Arc::new(tokio::sync::Mutex::new(reader_half));

    loop {
        // Check heartbeat timeout *before* entering the select so we don't
        // wait a full tick if we're already past the deadline.
        if last_pong.elapsed() > Duration::from_secs(HEARTBEAT_TIMEOUT_SECS) {
            emit_log(
                &events,
                Level::Warn,
                "LAN peer heartbeat timeout — closing session",
            )
            .await;
            return Err(anyhow!("heartbeat timeout"));
        }

        tokio::select! {
            // ── Cancellation ─────────────────────────────────────────────
            _ = cancel.cancelled() => {
                emit_log(&events, Level::Debug, "LAN peer session cancelled").await;
                return Ok(());
            }

            // ── Heartbeat tick ───────────────────────────────────────────
            _ = heartbeat_tick.tick() => {
                let ts = now_millis();
                let ping = PeerMessage::Ping { ts };
                let frame = encode_peer_message(&ping);
                let mut w = writer.lock().await;
                if let Err(e) = w.write_all(&frame).await {
                    return Err(anyhow!("failed to send ping: {}", e));
                }
            }

            // ── Outbound clipboard ───────────────────────────────────────
            outbound = rx_updates.recv() => {
                match outbound {
                    Ok(update) => {
                        let msg = PeerMessage::Clipboard {
                            content_type: update.payload.content_type.clone(),
                            data: update.payload.data.clone(),
                        };
                        let frame = encode_peer_message(&msg);
                        let mut w = writer.lock().await;
                        if let Err(e) = w.write_all(&frame).await {
                            return Err(anyhow!("failed to send clipboard: {}", e));
                        }
                        emit_log(
                            &events,
                            Level::Debug,
                            format!(
                                "LAN sent clipboard ({}) {} bytes",
                                update.payload.content_type,
                                update.payload.data.len()
                            ),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        emit_log(
                            &events,
                            Level::Warn,
                            format!("LAN outbound clipboard lagged by {} messages", n),
                        ).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // The clipboard monitor was shut down; exit gracefully.
                        return Ok(());
                    }
                }
            }

            // ── Inbound read ─────────────────────────────────────────────
            inbound = async {
                let mut r = reader.lock().await;
                read_peer_message_from_reader(&mut *r).await
            } => {
                match inbound {
                    Ok(msg) => {
                        match msg {
                            PeerMessage::Ping { ts } => {
                                let pong = PeerMessage::Pong { ts };
                                let frame = encode_peer_message(&pong);
                                let mut w = writer.lock().await;
                                if let Err(e) = w.write_all(&frame).await {
                                    return Err(anyhow!("failed to send pong: {}", e));
                                }
                            }
                            PeerMessage::Pong { .. } => {
                                last_pong = Instant::now();
                            }
                            PeerMessage::Clipboard { content_type, data } => {
                                let payload = ClipboardBroadcastPayload {
                                    content_type: content_type.clone(),
                                    data,
                                };
                                let _ = tx_in.send(payload).await;

                                let ct_label = match content_type.as_str() {
                                    CONTENT_TYPE_TEXT => "text",
                                    CONTENT_TYPE_IMAGE_PNG => "image",
                                    other => other,
                                };
                                emit_log(
                                    &events,
                                    Level::Info,
                                    format!("LAN received clipboard ({})", ct_label),
                                ).await;

                                let _ = events
                                    .send(RuntimeEvent::ClipboardReceived {
                                        content_type,
                                    })
                                    .await;
                            }
                            // Handshake messages arriving after the session has
                            // started are unexpected but not fatal — just log.
                            other => {
                                emit_log(
                                    &events,
                                    Level::Warn,
                                    format!(
                                        "LAN peer session: unexpected message type {:?}",
                                        msg_type_name(&other)
                                    ),
                                ).await;
                            }
                        }
                    }
                    Err(e) => {
                        // Connection closed or read error.
                        return Err(anyhow!("LAN peer read error: {}", e));
                    }
                }
            }
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Frame I/O helpers
// ────────────────────────────────────────────────────────────────────────────

/// Write a single length-prefixed JSON frame to `stream`.
async fn write_peer_message(stream: &mut TcpStream, msg: &PeerMessage) -> Result<()> {
    let frame = encode_peer_message(msg);
    stream.write_all(&frame).await.context("write_peer_message")
}

/// Read a single length-prefixed JSON frame from a `TcpStream`.
async fn read_peer_message(stream: &mut TcpStream) -> Result<PeerMessage> {
    // Read 4-byte length prefix.
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("reading frame length")?;
    let len = u32::from_be_bytes(len_buf);

    if len > MAX_FRAME_SIZE {
        return Err(anyhow!(
            "frame too large: {} bytes (max {})",
            len,
            MAX_FRAME_SIZE
        ));
    }

    let mut payload = vec![0u8; len as usize];
    stream
        .read_exact(&mut payload)
        .await
        .context("reading frame payload")?;

    serde_json::from_slice(&payload).context("deserialising PeerMessage")
}

/// Same as [`read_peer_message`] but works with an
/// [`tokio::net::tcp::OwnedReadHalf`] obtained via `stream.into_split()`.
async fn read_peer_message_from_reader(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
) -> Result<PeerMessage> {
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .context("reading frame length")?;
    let len = u32::from_be_bytes(len_buf);

    if len > MAX_FRAME_SIZE {
        return Err(anyhow!(
            "frame too large: {} bytes (max {})",
            len,
            MAX_FRAME_SIZE
        ));
    }

    let mut payload = vec![0u8; len as usize];
    reader
        .read_exact(&mut payload)
        .await
        .context("reading frame payload")?;

    serde_json::from_slice(&payload).context("deserialising PeerMessage")
}

// ────────────────────────────────────────────────────────────────────────────
// Tiny helpers
// ────────────────────────────────────────────────────────────────────────────

/// Emit a log event to the runtime channel (convenience wrapper).
async fn emit_log(events: &mpsc::Sender<RuntimeEvent>, level: Level, message: impl Into<String>) {
    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(level, message)))
        .await;
}

/// Return current time as milliseconds since the UNIX epoch.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Human-readable label for a [`PeerMessage`] variant (for log messages).
fn msg_type_name(msg: &PeerMessage) -> &'static str {
    match msg {
        PeerMessage::Hello { .. } => "Hello",
        PeerMessage::Welcome { .. } => "Welcome",
        PeerMessage::Ping { .. } => "Ping",
        PeerMessage::Pong { .. } => "Pong",
        PeerMessage::Clipboard { .. } => "Clipboard",
    }
}
