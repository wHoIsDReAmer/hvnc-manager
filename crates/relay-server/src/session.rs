use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use shared::LinkSide;
use tokio::sync::Mutex;

use crate::transport::PeerHandle;

pub type SessionId = u64;

#[derive(Default)]
pub struct SessionMap {
    inner: Mutex<State>,
    seq: AtomicU64,
}

#[derive(Default)]
struct State {
    waiting: HashMap<String, WaitingPeer>,
    paired: HashMap<SessionId, SessionEntry>,
}

struct WaitingPeer {
    side: LinkSide,
    peer: Arc<PeerHandle>,
}

struct SessionEntry {
    _token: String,
    manager: Arc<PeerHandle>,
    client: Arc<PeerHandle>,
}

impl SessionMap {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(State::default()),
            seq: AtomicU64::new(1),
        }
    }

    /// Lookup the counterpart peer for a given session and side.
    /// Returns None if session doesn't exist or peer side doesn't match.
    pub async fn get_counterpart(
        &self,
        session_id: SessionId,
        from_side: LinkSide,
    ) -> Option<Arc<PeerHandle>> {
        let guard = self.inner.lock().await;
        guard.paired.get(&session_id).map(|entry| match from_side {
            LinkSide::Manager => Arc::clone(&entry.client),
            LinkSide::Client => Arc::clone(&entry.manager),
        })
    }

    /// Remove a session when a peer disconnects.
    pub async fn remove_session(&self, session_id: SessionId) -> bool {
        let mut guard = self.inner.lock().await;
        guard.paired.remove(&session_id).is_some()
    }

    /// Remove a waiting peer by token.
    pub async fn remove_waiting(&self, token: &str) -> bool {
        let mut guard = self.inner.lock().await;
        guard.waiting.remove(token).is_some()
    }

    /// Register a peer by auth token and side.
    /// Returns (session_id, counterpart) where session_id is non-zero only when paired.
    pub async fn register(
        &self,
        token: String,
        side: LinkSide,
        peer: Arc<PeerHandle>,
    ) -> (SessionId, Option<Arc<PeerHandle>>) {
        let mut guard = self.inner.lock().await;
        if let Some(waiting) = guard.waiting.remove(&token) {
            if waiting.side == side {
                // same side waiting; replace and keep waiting
                guard.waiting.insert(
                    token,
                    WaitingPeer {
                        side,
                        peer: Arc::clone(&peer),
                    },
                );
                return (0, None);
            }
            let session_id = self.seq.fetch_add(1, Ordering::Relaxed);
            let (manager, client) = match side {
                LinkSide::Manager => (Arc::clone(&peer), waiting.peer),
                LinkSide::Client => (waiting.peer, Arc::clone(&peer)),
            };
            guard.paired.insert(
                session_id,
                SessionEntry {
                    _token: token,
                    manager: manager.clone(),
                    client: client.clone(),
                },
            );
            (
                session_id,
                Some(match side {
                    LinkSide::Manager => client,
                    LinkSide::Client => manager,
                }),
            )
        } else {
            guard.waiting.insert(
                token,
                WaitingPeer {
                    side,
                    peer: Arc::clone(&peer),
                },
            );
            (0, None)
        }
    }
}
