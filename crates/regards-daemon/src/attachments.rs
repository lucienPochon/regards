//! Received attachments: shown in the app, saved where the person chooses,
//! or copied for their program to open. What is written is marked as from
//! outside, as browsers mark downloads, so the system and programs such as
//! Office apply their checks (Protected View, macro blocking, quarantine).

use crate::State;
use crate::session::internal;
use regards_core::attachment::{self, Preview, Risk};
use regards_core::body;
use regards_ipc::protocol::{
    AttachmentFiles, AttachmentPreview, AttachmentRef, AttachmentSaveAllParams, AttachmentSaveParams, RpcError, codes,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

/// A message's attachments with their bytes, in the order `message.get` lists them.
type Parsed = Vec<(body::Attachment, body::File)>;
/// Filled once, by the first request for it.
type Slot = Arc<OnceCell<Arc<Parsed>>>;

/// Messages kept, at most.
const KEPT: usize = 4;
/// Bytes kept, at most, beyond the latest message's.
const KEPT_BYTES: usize = 96 * 1024 * 1024;

/// The attachments of the messages opened last, parsed once. A message's
/// tiles, its viewer and Save all ask for them one at a time, and parsing a
/// large message again for each costs as much as the first time. Keyed by
/// the stored message's hash: the same bytes always hold the same files.
#[derive(Default)]
pub(crate) struct Recent {
    entries: Mutex<VecDeque<(String, Slot)>>,
}

impl Recent {
    /// The entry for `hash`, made the newest; requests for it at the same
    /// time share one parse.
    fn entry(&self, hash: &str) -> Slot {
        let mut entries = self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let cell = match entries.iter().position(|(key, _)| key == hash) {
            Some(at) => entries.remove(at).expect("found just now").1,
            None => Arc::new(OnceCell::new()),
        };
        entries.push_front((hash.to_owned(), Arc::clone(&cell)));
        // The oldest go first, the newest stays whatever its size.
        let size = |cell: &OnceCell<Arc<Parsed>>| {
            cell.get().map_or(0, |parsed| {
                parsed.iter().map(|(_, file)| file.data.len()).sum::<usize>()
            })
        };
        while entries.len() > KEPT
            || (entries.len() > 1 && entries.iter().skip(1).map(|(_, cell)| size(cell)).sum::<usize>() > KEPT_BYTES)
        {
            entries.pop_back();
        }
        cell
    }
}

/// A message's attachments, downloading the message if needed.
async fn parsed(state: &State, message_id: i64) -> Result<Arc<Parsed>, RpcError> {
    let parse = || async move {
        let (_, source) = crate::mail::load(state, message_id).await?;
        tokio::task::spawn_blocking(move || Arc::new(body::attachments(&source)))
            .await
            .map_err(internal)
    };
    match crate::mail::stored_hash(state, message_id).await? {
        Some(hash) => state.attachments.entry(&hash).get_or_try_init(parse).await.cloned(),
        // Not downloaded yet: once it is, the next request finds its hash.
        None => parse().await,
    }
}

/// The attachment at `index` of a message.
async fn find(state: &State, message_id: i64, index: u32) -> Result<(Arc<Parsed>, usize), RpcError> {
    let parsed = parsed(state, message_id).await?;
    let index = index as usize;
    if index >= parsed.len() {
        return Err(RpcError::new(codes::NOT_FOUND, "no such attachment"));
    }
    Ok((parsed, index))
}

/// `attachment.preview`.
pub(crate) async fn preview(state: &State, params: AttachmentRef) -> Result<Value, RpcError> {
    let (parsed, index) = find(state, params.message_id, params.index).await?;
    let (described, file) = &parsed[index];
    let shown = match (described.preview, attachment::image_type(&file.data)) {
        (Some(Preview::Image), Some(mime)) => {
            if file.data.len() > AttachmentPreview::MAX_IMAGE {
                return Err(RpcError::new(
                    codes::TOO_LARGE,
                    "this image is too large to show here: open it or save it",
                ));
            }
            AttachmentPreview::Image {
                mime: mime.to_owned(),
                data: regards_core::encoding::base64(&file.data),
            }
        }
        (Some(Preview::Text), _) => {
            let data = &file.data;
            let mut end = data.len().min(AttachmentPreview::MAX_TEXT);
            // Not in the middle of a character.
            while end > 0 && end < data.len() && data[end] & 0xc0 == 0x80 {
                end -= 1;
            }
            AttachmentPreview::Text {
                text: String::from_utf8_lossy(&data[..end]).into_owned(),
                truncated: end < data.len(),
            }
        }
        _ => {
            return Err(RpcError::new(
                codes::INVALID_PARAMS,
                "this file can't be shown here: open it or save it",
            ));
        }
    };
    Ok(json!(shown))
}

/// `attachment.save`.
pub(crate) async fn save(state: &State, params: AttachmentSaveParams) -> Result<Value, RpcError> {
    let path = PathBuf::from(&params.path);
    if !path.is_absolute() {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the file's full path"));
    }
    let (parsed, index) = find(state, params.message_id, params.index).await?;
    tokio::task::spawn_blocking(move || {
        // The system's save dialog asked before replacing a file.
        std::fs::write(&path, &parsed[index].1.data).map_err(cannot_write)?;
        mark_from_outside(&path);
        Ok(json!(AttachmentFiles {
            paths: vec![path.to_string_lossy().into_owned()],
        }))
    })
    .await
    .map_err(internal)?
}

/// `attachment.save_all`: the files attached (not those shown inside the
/// message), never replacing one already in the folder.
pub(crate) async fn save_all(state: &State, params: AttachmentSaveAllParams) -> Result<Value, RpcError> {
    let folder = PathBuf::from(&params.folder);
    if !folder.is_absolute() {
        return Err(RpcError::new(codes::INVALID_PARAMS, "give the folder's full path"));
    }
    let parsed = parsed(state, params.message_id).await?;
    tokio::task::spawn_blocking(move || {
        if !folder.is_dir() {
            return Err(RpcError::new(codes::NOT_FOUND, "that folder does not exist"));
        }
        let mut paths = Vec::new();
        for (_, file) in parsed.iter().filter(|(described, _)| !described.inline) {
            let path = write_new(&folder, &file.name, &file.data).map_err(cannot_write)?;
            mark_from_outside(&path);
            paths.push(path.to_string_lossy().into_owned());
        }
        Ok(json!(AttachmentFiles { paths }))
    })
    .await
    .map_err(internal)?
}

/// `attachment.open`: a read-only copy in a folder of its own under
/// [`DataDirs::opened`](regards_core::paths::DataDirs::opened), for the app
/// to hand to the file's program.
pub(crate) async fn open(state: &State, params: AttachmentRef) -> Result<Value, RpcError> {
    let (parsed, index) = find(state, params.message_id, params.index).await?;
    let (described, _) = &parsed[index];
    if described.risk == Risk::SaveOnly {
        return Err(RpcError::new(
            codes::FORBIDDEN,
            "this file could run a program, so Regards does not open it: save it, and open it only if you trust it",
        ));
    }
    // Short: some programs still fail on paths over 260 characters.
    let folder = state
        .dirs
        .opened()
        .join(regards_core::oauth::random_token().chars().take(16).collect::<String>());
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&folder).map_err(cannot_write)?;
        let (_, file) = &parsed[index];
        let path = write_new(&folder, &file.name, &file.data).map_err(cannot_write)?;
        mark_from_outside(&path);
        // A copy: changes made to it would be lost, so say so up front.
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(true);
            let _ = std::fs::set_permissions(&path, permissions);
        }
        Ok(json!(AttachmentFiles {
            paths: vec![path.to_string_lossy().into_owned()],
        }))
    })
    .await
    .map_err(internal)?
}

/// Remove the copies opened before, when the daemon starts: their programs
/// have read them by then. One still in use stays until the next time.
pub(crate) fn sweep(opened: &Path) {
    /// Windows does not delete a read-only file (elsewhere the folder decides).
    #[cfg(windows)]
    fn writable(path: &Path) {
        let Ok(entries) = std::fs::read_dir(path) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                writable(&path);
            } else if let Ok(metadata) = std::fs::metadata(&path) {
                let mut permissions = metadata.permissions();
                // Only on Windows, where it clears the read-only attribute.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(&path, permissions);
            }
        }
    }
    if opened.exists() {
        #[cfg(windows)]
        writable(opened);
        let _ = std::fs::remove_dir_all(opened);
    }
}

/// Write `data` to `name` in `folder`, or to `name (2)`, `name (3)`… when
/// taken: never over an existing file.
fn write_new(folder: &Path, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    for n in 1..1000 {
        let path = if n == 1 {
            folder.join(name)
        } else {
            folder.join(format!("{stem} ({n}){extension}"))
        };
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        "too many files with that name",
    ))
}

fn cannot_write(error: std::io::Error) -> RpcError {
    RpcError::new(codes::INVALID_PARAMS, format!("cannot save the file: {error}"))
}

/// Mark a file as from outside, as browsers mark downloads. Best effort: a
/// drive without the means (FAT32, exFAT) leaves the file unmarked.
fn mark_from_outside(path: &Path) {
    #[cfg(windows)]
    {
        // The Internet zone, as Chromium writes when it marks a download itself.
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(PathBuf::from(stream), "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(target_os = "macos")]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let _ = std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine", &format!("0081;{now:x};Regards;")])
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(size: usize) -> Arc<Parsed> {
        let file = body::File {
            name: "a.bin".into(),
            mime: "application/octet-stream".into(),
            data: vec![0; size],
        };
        let described = body::Attachment {
            name: Some("a.bin".into()),
            mime: file.mime.clone(),
            size,
            inline: false,
            risk: Risk::Open,
            preview: None,
        };
        Arc::new(vec![(described, file)])
    }

    #[tokio::test]
    async fn recent_messages_are_parsed_once_and_the_oldest_go() {
        let recent = Recent::default();
        let first = recent.entry("a");
        assert!(Arc::ptr_eq(&first, &recent.entry("a")), "one slot per message");
        // Requests at the same time share one parse.
        let parses = std::sync::atomic::AtomicUsize::new(0);
        let parse = || async {
            parses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok::<_, RpcError>(filled(10))
        };
        let second = recent.entry("a");
        let (one, two) = tokio::join!(first.get_or_try_init(parse), second.get_or_try_init(parse));
        assert!(Arc::ptr_eq(one.unwrap(), two.unwrap()));
        assert_eq!(parses.load(std::sync::atomic::Ordering::SeqCst), 1);

        for hash in ["b", "c", "d", "e"] {
            recent.entry(hash);
        }
        assert!(!Arc::ptr_eq(&first, &recent.entry("a")), "past {KEPT}, the oldest went");
    }

    #[tokio::test]
    async fn recent_messages_keep_to_their_size_but_always_the_newest() {
        let recent = Recent::default();
        let big = recent.entry("big");
        big.set(filled(KEPT_BYTES)).unwrap();
        let huge = recent.entry("huge");
        huge.set(filled(KEPT_BYTES + 1)).unwrap();
        // The next one pushes the oversized older ones out; the newest stays.
        let small = recent.entry("small");
        assert!(Arc::ptr_eq(&small, &recent.entry("small")));
        assert!(!Arc::ptr_eq(&huge, &recent.entry("huge")), "over the budget, it went");
    }

    #[test]
    fn saving_never_replaces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = write_new(dir.path(), "report.pdf", b"1").unwrap();
        let second = write_new(dir.path(), "report.pdf", b"2").unwrap();
        let third = write_new(dir.path(), "notes", b"3").unwrap();
        let fourth = write_new(dir.path(), "notes", b"4").unwrap();
        assert_eq!(first.file_name().unwrap(), "report.pdf");
        assert_eq!(second.file_name().unwrap(), "report (2).pdf");
        assert_eq!(fourth.file_name().unwrap(), "notes (2)");
        assert_eq!(std::fs::read(&first).unwrap(), b"1");
        assert_eq!(std::fs::read(&third).unwrap(), b"3");
    }

    #[test]
    fn opened_copies_are_swept_even_when_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let opened = dir.path().join("opened");
        let folder = opened.join("abc");
        std::fs::create_dir_all(&folder).unwrap();
        let path = write_new(&folder, "a.txt", b"x").unwrap();
        mark_from_outside(&path);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        sweep(&opened);
        assert!(!opened.exists());
        sweep(&opened);
    }

    #[cfg(windows)]
    #[test]
    fn saved_files_are_marked_as_from_the_internet() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_new(dir.path(), "a.docx", b"x").unwrap();
        mark_from_outside(&path);
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let mark = std::fs::read_to_string(PathBuf::from(stream)).unwrap();
        assert_eq!(mark, "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
}
