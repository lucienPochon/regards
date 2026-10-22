//! The switch for every agent at once (Settings › Agents). Off, the CLI and
//! MCP get nothing but `hello`, whatever each account allows; the accounts'
//! own levels stay as they are, for when it is on again. Agents connected
//! now can also be sent away. `ui` only.

use crate::State;
use crate::accounts::store_error;
use regards_ipc::protocol::{AgentsDisconnected, AgentsEnabledParams, AgentsSettings, RpcError, codes};
use regards_store::settings;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::watch;

/// The daemon's key for the switch: "off", or on.
pub(crate) const KEY: &str = "agents.enabled";

pub(crate) struct Gate {
    enabled: AtomicBool,
    /// Moved on to end every agent connection: each one holds a receiver.
    kick: watch::Sender<u64>,
}

impl Gate {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            kick: watch::Sender::new(0),
        }
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
