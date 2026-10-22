//! The switch for every agent at once (Settings › Agents). Off, the CLI and
//! MCP get nothing but `hello`, whatever each account allows; the accounts'
//! own levels stay as they are, for when it is on again. Agents connected
//! now can also be sent away. `ui` only.

use crate::State;
use crate::accounts::store_error;
use regards_ipc::protocol::{AgentsDisconnected, AgentsEnabledParams, AgentsSettings, RpcError, codes};
use regards_store::settings;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::watch;

/// The daemon's key for the switch: "off", or on.
pub(crate) const KEY: &str = "agents.enabled";

pub(crate) struct Gate {
    enabled: AtomicBool,
    /// Moved on to end every agent connection: each one holds a receiver.
    kick: watch::Sender<u64>,
    /// The apps agents connected now run in, as they name themselves, by connection.
    apps: Mutex<Vec<(u64, String)>>,
    next_app: AtomicU64,
}

/// An agent connection's app, listed while the connection lasts.
pub(crate) struct AppSeen {
    state: Arc<State>,
    id: u64,
}

impl Drop for AppSeen {
    fn drop(&mut self) {
        let mut apps = self.state.agents.apps.lock().unwrap_or_else(PoisonError::into_inner);
        apps.retain(|(id, _)| *id != self.id);
    }
}

/// The longest app name kept.
const APP_NAME_MAX: usize = 100;

impl Gate {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            kick: watch::Sender::new(0),
            apps: Mutex::new(Vec::new()),
            next_app: AtomicU64::new(1),
        }
    }

    /// An agent connection says which app it runs in; listed until the
    /// guard is dropped with the connection. Nothing for a blank name.
    pub(crate) fn seen(state: &Arc<State>, app: &str) -> Option<AppSeen> {
        let name: String = app.trim().chars().take(APP_NAME_MAX).collect();
        if name.is_empty() {
            return None;
        }
        let id = state.agents.next_app.fetch_add(1, Ordering::SeqCst);
        state
            .agents
            .apps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id, name));
        Some(AppSeen {
            state: Arc::clone(state),
            id,
        })
    }

    /// The apps connected now, each once, sorted.
    fn apps(&self) -> Vec<String> {
        let apps = self.apps.lock().unwrap_or_else(PoisonError::into_inner);
        let mut names: Vec<String> = apps.iter().map(|(_, name)| name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// For an agent's connection: changes when it must end.
    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.kick.subscribe()
    }

    /// Agents connected now.
    pub(crate) fn connected(&self) -> usize {
        self.kick.receiver_count()
    }

    /// End every agent connection now; returns how many there were.
    pub(crate) fn disconnect(&self) -> usize {
        let connected = self.connected();
        self.kick.send_modify(|generation| *generation += 1);
        connected
    }
}

/// What an agent is told while the switch is off.
pub(crate) fn turned_off() -> RpcError {
    RpcError::new(codes::FORBIDDEN, "agents are turned off in Regards (Settings › Agents)")
}

fn view(state: &State) -> AgentsSettings {
    AgentsSettings {
        enabled: state.agents.enabled(),
        connected: u32::try_from(state.agents.connected()).unwrap_or(u32::MAX),
        apps: state.agents.apps(),
    }
}

/// `agents.settings`.
pub(crate) fn settings(state: &State) -> Value {
    json!(view(state))
}

/// `agents.set_enabled`: off also sends away the agents connected now.
pub(crate) async fn set_enabled(state: &State, params: AgentsEnabledParams) -> Result<Value, RpcError> {
    let value = if params.enabled { "on" } else { "off" };
    state
        .store
        .write(move |tx| settings::set(tx, KEY, value))
        .await
        .map_err(store_error)?;
    state.agents.enabled.store(params.enabled, Ordering::SeqCst);
    if !params.enabled {
        let ended = state.agents.disconnect();
        tracing::info!(ended, "agents turned off");
    }
    Ok(json!(view(state)))
}

/// `agents.disconnect`.
pub(crate) fn disconnect(state: &State) -> Value {
    let disconnected = state.agents.disconnect();
    tracing::info!(disconnected, "agents sent away");
    json!(AgentsDisconnected {
        disconnected: u32::try_from(disconnected).unwrap_or(u32::MAX),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sending_agents_away_reaches_each_connection() {
        let gate = Gate::new(true);
        let mut one = gate.subscribe();
        let two = gate.subscribe();
        assert_eq!(gate.connected(), 2);
        drop(two);
        assert_eq!(gate.disconnect(), 1);
        assert!(one.changed().await.is_ok(), "the connection hears it");
    }
}
