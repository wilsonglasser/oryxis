//! In-memory FIFO queue per recipient.
//!
//! Each entry is `(sender_id, bytes, inserted_at)`. Entries TTL out
//! after `RELAY_TTL` to mirror the Worker. A restart drops every
//! queue; the engine will retry naturally on the next `Sync Now` or
//! mDNS rediscovery, so we accept the data loss in exchange for
//! zero deployment surface (no SQLite, no migrations).
//!
//! Bound the total memory footprint with `MAX_QUEUE_DEPTH` per
//! recipient: once exceeded the oldest entry is dropped silently
//! (clients can resync any time).

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

/// How long an undelivered frame survives in the queue. Mirrors the
/// Worker's `TTL = 300s`.
pub const RELAY_TTL: Duration = Duration::from_secs(300);

/// Soft cap on per-recipient queue depth. The engine sends in tight
/// request/response pairs, so the steady-state depth is 1-2; this is
/// just a defense against a runaway sender.
const MAX_QUEUE_DEPTH: usize = 256;

#[derive(Clone)]
pub struct QueueEntry {
    pub sender_id: Uuid,
    pub body: Bytes,
    pub inserted_at: Instant,
}

/// Lookup for `peer_id` -> the `Notify` its long-poll consumer is
/// waiting on. The producer raises the notify after pushing so a
/// pending consumer wakes without polling. Wrapped in `Arc<Mutex>`
/// (small map, low contention) rather than `DashMap` to avoid an
/// extra dep.
#[derive(Default, Clone)]
pub struct InboxRegistry {
    inner: Arc<Mutex<HashMap<Uuid, Inbox>>>,
}

#[derive(Default)]
struct Inbox {
    queue: VecDeque<QueueEntry>,
    /// Shared so consumers can `.notified()` while we still hold the
    /// map lock.
    notify: Arc<Notify>,
}

impl InboxRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a frame into `recipient`'s inbox and wake any consumer.
    /// Returns the new depth so the caller can log overflow drops.
    pub async fn push(
        &self,
        recipient: Uuid,
        entry: QueueEntry,
    ) -> usize {
        let mut map = self.inner.lock().await;
        let inbox = map.entry(recipient).or_default();
        if inbox.queue.len() >= MAX_QUEUE_DEPTH {
            // Drop oldest. A correct client will resync.
            inbox.queue.pop_front();
        }
        inbox.queue.push_back(entry);
        let depth = inbox.queue.len();
        inbox.notify.notify_one();
        depth
    }

    /// Pop the oldest non-expired entry, or wait up to `wait` for one
    /// to arrive. Returns `None` after the deadline with the queue
    /// still empty (long-poll 204 response).
    pub async fn pop_wait(
        &self,
        recipient: Uuid,
        wait: Duration,
    ) -> Option<QueueEntry> {
        let deadline = Instant::now() + wait;
        loop {
            let notify = {
                let mut map = self.inner.lock().await;
                let inbox = map.entry(recipient).or_default();
                // Drop expired entries lazily before serving.
                while let Some(front) = inbox.queue.front() {
                    if front.inserted_at.elapsed() > RELAY_TTL {
                        inbox.queue.pop_front();
                    } else {
                        break;
                    }
                }
                if let Some(entry) = inbox.queue.pop_front() {
                    return Some(entry);
                }
                inbox.notify.clone()
            };
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let remaining = deadline - now;
            if tokio::time::timeout(remaining, notify.notified()).await.is_err() {
                return None;
            }
        }
    }

    /// Background sweeper: drop every expired entry across every
    /// inbox once per `interval`. Without this, an inbox whose
    /// consumer never returns would hold its TTL'd frames until next
    /// access.
    pub fn spawn_sweeper(self, interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(
                tokio::time::MissedTickBehavior::Delay,
            );
            // Skip the immediate first tick so the sweeper doesn't
            // run before the server even bound its port.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let mut map = self.inner.lock().await;
                for inbox in map.values_mut() {
                    while let Some(front) = inbox.queue.front() {
                        if front.inserted_at.elapsed() > RELAY_TTL {
                            inbox.queue.pop_front();
                        } else {
                            break;
                        }
                    }
                }
                map.retain(|_, inbox| !inbox.queue.is_empty());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn push_and_pop_round_trip() {
        let reg = InboxRegistry::new();
        let recipient = Uuid::new_v4();
        let sender = Uuid::new_v4();
        reg.push(recipient, QueueEntry {
            sender_id: sender,
            body: Bytes::from_static(b"hello"),
            inserted_at: Instant::now(),
        }).await;
        let got = reg.pop_wait(recipient, Duration::from_millis(100)).await;
        let entry = got.expect("entry present");
        assert_eq!(entry.sender_id, sender);
        assert_eq!(&entry.body[..], b"hello");
    }

    #[tokio::test]
    async fn pop_returns_none_after_wait() {
        let reg = InboxRegistry::new();
        let got = reg.pop_wait(Uuid::new_v4(), Duration::from_millis(50)).await;
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn push_wakes_pending_consumer() {
        let reg = InboxRegistry::new();
        let recipient = Uuid::new_v4();
        let sender = Uuid::new_v4();

        let reg2 = reg.clone();
        let consumer = tokio::spawn(async move {
            reg2.pop_wait(recipient, Duration::from_secs(2)).await
        });

        // Give consumer time to register its notify.
        tokio::time::sleep(Duration::from_millis(50)).await;

        reg.push(recipient, QueueEntry {
            sender_id: sender,
            body: Bytes::from_static(b"x"),
            inserted_at: Instant::now(),
        }).await;

        let got = consumer.await.unwrap();
        assert!(got.is_some());
    }

    #[tokio::test]
    async fn overflow_drops_oldest() {
        let reg = InboxRegistry::new();
        let recipient = Uuid::new_v4();
        let sender = Uuid::new_v4();
        for i in 0..(MAX_QUEUE_DEPTH + 5) {
            reg.push(recipient, QueueEntry {
                sender_id: sender,
                body: Bytes::from(vec![i as u8]),
                inserted_at: Instant::now(),
            }).await;
        }
        // The first 5 should have been dropped.
        let first = reg
            .pop_wait(recipient, Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(first.body[0], 5);
    }
}
