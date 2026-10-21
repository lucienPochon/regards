//! "Remove everything from this computer" (Support): every credential
//! Regards kept goes at once. Then the daemon stops, leaving a marker, so its
//! files can go once nothing holds them: the app removes the data directory
//! as soon as the daemon has exited, and a daemon that starts and finds the
//! marker finishes the job before opening anything. `ui` only.

use crate::State;
use crate::accounts::{store_error, vault, vault_error};
use regards_core::paths::DataDirs;
use regards_ipc::protocol::{DaemonEraseParams, DaemonErased, RpcError, codes};
use regards_store::accounts;
use regards_vault::SecretKey;
use serde_json::{Value, json};
use std::path::Path;

/// `daemon.erase`: the credentials now; the files once the daemon has
/// stopped. The caller stops the daemon after replying.
pub(crate) async fn erase(state: &State, params: DaemonEraseParams) -> Result<Value, RpcError> {
    if params.confirm != DaemonEraseParams::CONFIRMATION {
        return Err(RpcError::new(
            codes::INVALID_PARAMS,
            format!(
                "to erase everything, confirm with \"{}\"",
                DaemonEraseParams::CONFIRMATION
            ),
        ));
    }
    let mut keys: Vec<SecretKey> = state
        .store
        .read(accounts::list)
        .await
        .map_err(store_error)?
        .into_iter()
        .map(|account| SecretKey::from_stored(account.secret_key))
        .collect();
    keys.push(crate::assistant::key());
    // Without a credential store, nothing was ever kept in one.
    if let Ok(vault) = vault(state).await {
        for key in &keys {
            // Nothing else goes if a password would stay behind.
            vault.delete(key).await.map_err(|error| {
                let mut refused = vault_error(error);
                refused.message = format!(
                    "the system credential store kept a password, so nothing was removed: {}",
                    refused.message
                );
                refused
            })?;
        }
    }
    std::fs::write(state.dirs.erase_marker(), b"").map_err(|error| {
        RpcError::new(
            codes::INTERNAL_ERROR,
            format!("could not mark the data for removal: {error}"),
        )
    })?;
    state.watchers.stop_all();
    state.backfills.stop_all();
    tracing::info!("everything is to be removed; stopping");
    Ok(json!(DaemonErased {
        credentials: u32::try_from(keys.len()).unwrap_or(u32::MAX),
    }))
}

/// At start, before anything is opened: when the last daemon was asked to
/// erase everything, remove what is left of it (all but the lock just
/// taken). The marker goes last, so a removal that fails is tried again.
pub(crate) fn finish(dirs: &DataDirs) {
    let marker = dirs.erase_marker();
    if !marker.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dirs.data()) else {
        return;
    };
    let mut left = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path == dirs.lock_file() || path == marker {
            continue;
        }
        if remove(&path).is_err() {
            left += 1;
        }
    }
    if left == 0 {
        let _ = std::fs::remove_file(&marker);
        tracing::info!("everything Regards kept was removed");
    } else {
        tracing::warn!(left, "some of what Regards kept could not be removed yet");
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_start_removes_what_is_left_but_its_own_lock() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(temp.path());
        let file = |name: &str| {
            let path = temp.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        };
        for name in [
            "mail.db",
            "search.db",
            "desktop.json",
            "blobs/ab/cd",
            "logs/daemon.log",
            "daemon.lock",
        ] {
            file(name);
        }
        finish(&dirs);
        assert!(temp.path().join("mail.db").exists(), "nothing goes without the marker");

        std::fs::write(dirs.erase_marker(), b"").unwrap();
        finish(&dirs);
        let left: Vec<String> = std::fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["daemon.lock"]);
    }
}
