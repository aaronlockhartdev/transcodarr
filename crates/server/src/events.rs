//! The live-update bus (DESIGN §9.0): every client holds one
//! `/api/events` SSE connection. The server pushes typed events when
//! state changes; clients fetch the initial snapshot over the regular
//! JSON endpoints and the stream carries only deltas from there.
//!
//! Stream positions are monotonic `id`s. A client that (re)connects
//! echoes its last seen position back as `Last-Event-ID`, and the
//! handler replays the in-memory ring (the most recent [`RING_CAP`]
//! events) before joining the live feed — so a dropped-and-reopened
//! connection (or a mid-job page load) resumes without losing events.
//! If a *connected* client falls behind the broadcast channel's own
//! capacity, the stream emits a `resync` frame and the client does a
//! full refetch instead of guessing.
//!
//! The bus is process-global, in-memory, and deliberately tiny: this
//! is a single-node LAN tool, not an event-sourcing platform.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::response::sse::Event;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};

/// How many past events the replay ring holds (reconnect backfill).
pub const RING_CAP: usize = 1024;

/// Per-subscriber broadcast capacity. A client that cannot keep up
/// with this is `Lagged`, which the stream turns into a `resync`
/// frame (full refetch) rather than silently dropped state.
const CHANNEL_CAP: usize = 256;

/// How often the handler sends an SSE keep-alive comment (keeps idle
/// proxies and NATs from reaping the connection).
pub const KEEPALIVE: Duration = Duration::from_secs(15);

/// One typed event on the live feed.
///
/// Most variants are *nudges* carrying only ids: the client refetches
/// the authoritative row(s) over JSON. Keeping the payload id-only
/// means the wire shape can never drift from the DB row shapes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// A job row changed (queued, claimed, finished, reclaimed, or
    /// superseded). The client refetches the job list.
    JobChanged {
        /// The job that changed.
        job_id: i64,
    },
    /// A batched (~200 ms) chunk of a running job's ffmpeg log.
    /// The only high-volume event; job-detail viewers append it live.
    JobLog {
        /// The job whose log this is.
        job_id: i64,
        /// New log text (already line-oriented).
        chunk: String,
    },
    /// A file row changed (scan or re-evaluation). The client
    /// refetches that library's files.
    FileChanged {
        /// The file that changed.
        file_id: i64,
        /// Its library (the client's fetch unit).
        library_id: i64,
    },
    /// A library was created, updated, or deleted. The client
    /// refetches core state (libraries + flows).
    LibraryChanged {
        /// The library that changed.
        library_id: i64,
    },
    /// A flow was created, updated, or deleted. The client refetches
    /// core state.
    FlowChanged {
        /// The flow that changed.
        flow_id: i64,
    },
    /// Liveness heartbeat, emitted on a fixed cadence even when
    /// nothing else is happening.
    Tick,
}

impl ServerEvent {
    /// The SSE `event:` name for this variant.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::JobChanged { .. } => "job_changed",
            Self::JobLog { .. } => "job_log",
            Self::FileChanged { .. } => "file_changed",
            Self::LibraryChanged { .. } => "library_changed",
            Self::FlowChanged { .. } => "flow_changed",
            Self::Tick => "tick",
        }
    }
}

/// A ring/broadcast entry: the event plus its stream position.
#[derive(Clone, Debug)]
pub struct Tagged {
    /// Monotonic position in the global stream (1-based; `0` means
    /// "no events yet").
    pub id: u64,
    /// The event itself.
    pub event: ServerEvent,
}

/// The process-wide pub/sub for live updates.
///
/// Send+Sync; share it as `Arc<EventBus>` through [`AppState`].
/// `emit` is cheap (one mutex op, one broadcast send) and may be
/// called from any thread, including blocking job workers.
pub struct EventBus {
    tx: broadcast::Sender<Tagged>,
    seq: AtomicU64,
    ring: Mutex<VecDeque<Tagged>>,
}

impl EventBus {
    /// Create an empty bus (no subscribers, empty ring).
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAP);
        Self {
            tx,
            seq: AtomicU64::new(0),
            ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
        }
    }

    /// Publish one event: assigns the next stream id, keeps it in the
    /// replay ring, and pushes it to live subscribers.
    ///
    /// A send with zero live subscribers drops only the live copy —
    /// the ring still covers a later (re)connect. Returns the id.
    pub fn emit(&self, event: ServerEvent) -> u64 {
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let tagged = Tagged { id, event };
        {
            let mut ring = self.ring.lock().unwrap();
            if ring.len() == RING_CAP {
                ring.pop_front();
            }
            ring.push_back(tagged.clone());
        }
        let _ = self.tx.send(tagged);
        id
    }

    /// Subscribe for live events (broadcast semantics: each receiver
    /// sees events from the moment of subscription onward).
    pub fn subscribe(&self) -> broadcast::Receiver<Tagged> {
        self.tx.subscribe()
    }

    /// Ring entries with `id > after`, oldest first — the reconnect
    /// backfill. `after = 0` replays everything still retained.
    pub fn replay_since(&self, after: u64) -> Vec<Tagged> {
        self.ring
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.id > after)
            .cloned()
            .collect()
    }

    /// The latest published id (`0` if nothing has been published).
    pub fn latest_id(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Terminal (unused) error type for [`SseStream`]; the stream ends
/// with `None` instead of `Err`.
#[derive(Error, Debug)]
#[error("SSE stream ended")]
pub struct SseStreamEnd;

/// One message the pump thread hands to the stream.
enum PumpMsg {
    /// A live event off the broadcast feed.
    Event(Tagged),
    /// The receiver lagged: the client missed more events than the
    /// channel retains and must full-refetch.
    Lagged,
}

/// The stream behind the SSE handler: retained replay first, then
/// the live feed.
///
/// The live half is bridged by a small *pump thread*: tokio's
/// broadcast `recv()` future is `!Unpin` (it wraps tokio's
/// cooperative projection) and borrows the receiver, so neither it
/// nor the receiver can be stored in a `Stream` impl. The pump
/// thread calls `blocking_recv` on its own receiver and forwards the
/// outcome over an unbounded mpsc, which the stream polls via a
/// per-poll `Box::pin`. One OS thread per connected client is a
/// fair trade for a single-node LAN tool; dropping the stream
/// closes the mpsc and ends the thread.
///
/// A `Lagged` outcome yields one `resync` frame and then continues
/// live — the receiver re-synchronizes to the newest event
/// automatically, and the client is expected to full-refetch after a
/// `resync`. `Closed` (the bus dropped, i.e. process shutdown) ends
/// the stream cleanly.
pub struct SseStream {
    rx: mpsc::UnboundedReceiver<PumpMsg>,
    replay: VecDeque<Tagged>,
}

impl SseStream {
    /// Wrap a fresh subscription plus its replay backfill, spawning
    /// the pump thread for the subscription.
    pub fn new(rx: broadcast::Receiver<Tagged>, replay: VecDeque<Tagged>) -> Self {
        let (tx, rx2) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let mut rx = rx;
            loop {
                let msg = match rx.blocking_recv() {
                    Ok(t) => PumpMsg::Event(t),
                    Err(broadcast::error::RecvError::Lagged(_)) => PumpMsg::Lagged,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                // A send error means the stream (its HTTP connection)
                // is gone: stop pumping.
                if tx.send(msg).is_err() {
                    return;
                }
            }
        });
        Self { rx: rx2, replay }
    }
}

/// The transport-level `resync` frame name (never a ring event).
pub const RESYNC: &str = "resync";

impl Stream for SseStream {
    type Item = Result<Event, SseStreamEnd>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        // Replay backlog first (Last-Event-ID catch-up), then the
        // live feed.
        if let Some(tagged) = this.replay.pop_front() {
            return Poll::Ready(Some(Ok(frame(tagged))));
        }
        // The mpsc `recv` future borrows the receiver for this poll
        // only; `Box::pin` handles its `!Unpin` projection (one
        // allocation per idle poll — negligible at this volume).
        let mut fut = Box::pin(this.rx.recv());
        match fut.as_mut().poll(cx) {
            Poll::Ready(Some(PumpMsg::Event(tagged))) => Poll::Ready(Some(Ok(frame(tagged)))),
            Poll::Ready(Some(PumpMsg::Lagged)) => {
                Poll::Ready(Some(Ok(Event::default().event(RESYNC))))
            }
            // Channel closed: the pump thread saw the bus close (or
            // this stream was dropped first). End cleanly either way.
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// The SSE keep-alive wrapper around a bus stream.
pub type SseResponse = axum::response::Sse<axum::response::sse::KeepAliveStream<SseStream>>;

fn frame(tagged: Tagged) -> Event {
    let data = serde_json::to_string(&tagged.event).unwrap_or_else(|_| "{}".into());
    Event::default()
        .event(tagged.event.type_name())
        .id(tagged.id.to_string())
        .data(data)
}

/// Build the shared bus instance for one server process.
pub fn bus() -> Arc<EventBus> {
    Arc::new(EventBus::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_assigns_monotonic_ids() {
        let bus = EventBus::new();
        assert_eq!(bus.latest_id(), 0);
        let a = bus.emit(ServerEvent::Tick);
        let b = bus.emit(ServerEvent::Tick);
        let c = bus.emit(ServerEvent::Tick);
        assert_eq!((a, b, c), (1, 2, 3));
        assert_eq!(bus.latest_id(), 3);
    }

    #[test]
    fn replay_since_is_inclusive_of_newer_only() {
        let bus = EventBus::new();
        for i in 0..5 {
            bus.emit(ServerEvent::Tick);
            let _ = i;
        }
        assert_eq!(bus.replay_since(0).len(), 5);
        let mid = bus.replay_since(2);
        assert_eq!(mid.len(), 3);
        assert_eq!(mid[0].id, 3);
        assert_eq!(mid[2].id, 5);
        assert!(bus.replay_since(5).is_empty());
        // Beyond the newest id: nothing.
        assert!(bus.replay_since(99).is_empty());
    }

    #[test]
    fn ring_evicts_oldest_beyond_cap() {
        let bus = EventBus::new();
        let n = RING_CAP + 10;
        for _ in 0..n {
            bus.emit(ServerEvent::Tick);
        }
        let replay = bus.replay_since(0);
        assert_eq!(replay.len(), RING_CAP);
        assert_eq!(replay[0].id, 11); // ids 1..=10 evicted
        assert_eq!(replay[0].event, ServerEvent::Tick);
    }

    #[test]
    fn event_wire_shape_is_tagged_and_snake_cased() {
        let v: serde_json::Value = serde_json::to_value(ServerEvent::FileChanged {
            file_id: 7,
            library_id: 3,
        })
        .unwrap();
        assert_eq!(v["type"], "file_changed");
        assert_eq!(v["file_id"], 7);
        assert_eq!(v["library_id"], 3);
    }

    #[test]
    fn type_names_match_serde_tags() {
        for e in [
            ServerEvent::JobChanged { job_id: 1 },
            ServerEvent::JobLog {
                job_id: 1,
                chunk: "x".into(),
            },
            ServerEvent::FileChanged {
                file_id: 1,
                library_id: 1,
            },
            ServerEvent::LibraryChanged { library_id: 1 },
            ServerEvent::FlowChanged { flow_id: 1 },
            ServerEvent::Tick,
        ] {
            let v = serde_json::to_value(&e).unwrap();
            assert_eq!(v["type"].as_str().unwrap(), e.type_name());
        }
    }
}
