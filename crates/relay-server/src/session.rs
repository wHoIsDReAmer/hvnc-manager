use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use shared::LinkSide;
use shared::protocol::{ClientId, ClientInfo, SessionId};
use tokio::sync::RwLock;

use crate::transport::PeerHandle;

pub type ManagerId = u64;

pub struct SessionManager {
    inner: RwLock<State>,
    client_id_seq: AtomicU64,
    session_id_seq: AtomicU64,
}

#[derive(Default)]
struct State {
    clients: HashMap<ClientId, ClientEntry>,
    managers: HashMap<ManagerId, ManagerEntry>,
    sessions: HashMap<SessionId, ActiveSession>,
}

struct ClientEntry {
    info: ClientInfo,
    peer: Arc<PeerHandle>,
    active_session: Option<SessionId>,
}

struct ManagerEntry {
    node_name: String,
    peer: Arc<PeerHandle>,
    active_session: Option<SessionId>,
}

struct ActiveSession {
    manager_id: ManagerId,
    client_id: ClientId,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(State::default()),
            client_id_seq: AtomicU64::new(1),
            session_id_seq: AtomicU64::new(1),
        }
    }

    fn next_client_id(&self) -> ClientId {
        self.client_id_seq.fetch_add(1, Ordering::Relaxed)
    }

    fn next_session_id(&self) -> SessionId {
        self.session_id_seq.fetch_add(1, Ordering::Relaxed)
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub async fn register_client(&self, node_name: String, peer: Arc<PeerHandle>) -> ClientId {
        let client_id = self.next_client_id();
        let info = ClientInfo {
            client_id,
            node_name,
            connected_at: Self::now_ms(),
            is_busy: false,
        };

        let entry = ClientEntry {
            info,
            peer,
            active_session: None,
        };

        self.inner.write().await.clients.insert(client_id, entry);
        client_id
    }

    pub async fn unregister_client(&self, client_id: ClientId) -> Option<SessionId> {
        let mut state = self.inner.write().await;

        if let Some(entry) = state.clients.remove(&client_id)
            && let Some(session_id) = entry.active_session
            && let Some(session) = state.sessions.remove(&session_id)
            && let Some(manager) = state.managers.get_mut(&session.manager_id)
        {
            manager.active_session = None;
            return Some(session_id);
        }
        None
    }

    pub async fn get_client(&self, client_id: ClientId) -> Option<ClientInfo> {
        self.inner
            .read()
            .await
            .clients
            .get(&client_id)
            .map(|e| e.info.clone())
    }

    pub async fn register_manager(&self, node_name: String, peer: Arc<PeerHandle>) -> ManagerId {
        let manager_id = self.client_id_seq.fetch_add(1, Ordering::Relaxed);
        let entry = ManagerEntry {
            node_name,
            peer,
            active_session: None,
        };

        self.inner.write().await.managers.insert(manager_id, entry);
        manager_id
    }

    pub async fn unregister_manager(&self, manager_id: ManagerId) -> Option<SessionId> {
        let mut state = self.inner.write().await;

        if let Some(entry) = state.managers.remove(&manager_id)
            && let Some(session_id) = entry.active_session
        {
            if let Some(session) = state.sessions.remove(&session_id)
                && let Some(client) = state.clients.get_mut(&session.client_id)
            {
                client.active_session = None;
                client.info.is_busy = false;
            }
            return Some(session_id);
        }
        None
    }

    pub async fn list_clients(&self) -> Vec<ClientInfo> {
        self.inner
            .read()
            .await
            .clients
            .values()
            .map(|e| e.info.clone())
            .collect()
    }

    pub async fn get_all_manager_peers(&self) -> Vec<Arc<PeerHandle>> {
        self.inner
            .read()
            .await
            .managers
            .values()
            .map(|e| Arc::clone(&e.peer))
            .collect()
    }

    pub async fn connect(
        &self,
        manager_id: ManagerId,
        target_client_id: ClientId,
    ) -> Result<(SessionId, Arc<PeerHandle>, String), ConnectError> {
        let mut state = self.inner.write().await;

        let manager = state
            .managers
            .get(&manager_id)
            .ok_or(ConnectError::ManagerNotFound)?;
        if manager.active_session.is_some() {
            return Err(ConnectError::ManagerAlreadyInSession);
        }

        let client = state
            .clients
            .get(&target_client_id)
            .ok_or(ConnectError::ClientNotFound)?;
        if client.info.is_busy {
            return Err(ConnectError::ClientBusy);
        }

        let session_id = self.next_session_id();
        let client_peer = Arc::clone(&client.peer);
        let client_name = client.info.node_name.clone();

        let client = state.clients.get_mut(&target_client_id).unwrap();
        client.active_session = Some(session_id);
        client.info.is_busy = true;

        let manager = state.managers.get_mut(&manager_id).unwrap();
        manager.active_session = Some(session_id);

        state.sessions.insert(
            session_id,
            ActiveSession {
                manager_id,
                client_id: target_client_id,
            },
        );

        Ok((session_id, client_peer, client_name))
    }

    pub async fn disconnect(
        &self,
        manager_id: ManagerId,
    ) -> Result<(ClientId, Arc<PeerHandle>), DisconnectError> {
        let mut state = self.inner.write().await;

        let manager = state
            .managers
            .get(&manager_id)
            .ok_or(DisconnectError::ManagerNotFound)?;
        let session_id = manager
            .active_session
            .ok_or(DisconnectError::NotInSession)?;

        let session = state
            .sessions
            .remove(&session_id)
            .ok_or(DisconnectError::SessionNotFound)?;
        let client_id = session.client_id;

        let client_peer = state
            .clients
            .get(&client_id)
            .map(|c| Arc::clone(&c.peer))
            .ok_or(DisconnectError::ClientNotFound)?;

        if let Some(client) = state.clients.get_mut(&client_id) {
            client.active_session = None;
            client.info.is_busy = false;
        }

        if let Some(manager) = state.managers.get_mut(&manager_id) {
            manager.active_session = None;
        }

        Ok((client_id, client_peer))
    }

    pub async fn get_session_counterpart(
        &self,
        from_side: LinkSide,
        peer_id: u64,
    ) -> Option<Arc<PeerHandle>> {
        let state = self.inner.read().await;

        match from_side {
            LinkSide::Client => {
                let client = state.clients.get(&peer_id)?;
                let session_id = client.active_session?;
                let session = state.sessions.get(&session_id)?;
                let manager = state.managers.get(&session.manager_id)?;
                Some(Arc::clone(&manager.peer))
            }
            LinkSide::Manager => {
                let manager = state.managers.get(&peer_id)?;
                let session_id = manager.active_session?;
                let session = state.sessions.get(&session_id)?;
                let client = state.clients.get(&session.client_id)?;
                Some(Arc::clone(&client.peer))
            }
        }
    }

    pub async fn get_manager_name(&self, manager_id: ManagerId) -> Option<String> {
        self.inner
            .read()
            .await
            .managers
            .get(&manager_id)
            .map(|m| m.node_name.clone())
    }
}

#[derive(Debug, Clone)]
pub enum ConnectError {
    ManagerNotFound,
    ManagerAlreadyInSession,
    ClientNotFound,
    ClientBusy,
}

#[derive(Debug, Clone)]
pub enum DisconnectError {
    ManagerNotFound,
    NotInSession,
    SessionNotFound,
    ClientNotFound,
}
