//! What a list row shows beyond the headers: the start of the text (preview)
//! and whether files go with it (the paperclip). The files' names and types
//! are kept too, for search (`filename:`, `has:pdf`). Each message's structure
//! (BODYSTRUCTURE) says which part holds its text; then the first octets of
//! that part are read, a FETCH per kind of part. Best effort: a server that
//! cannot say leaves rows without a preview.
//!
//! A sync does this for the newest of what it brought in
//! ([`AT_SYNC`]); the backfill does the rest, newest first.

use crate::State;
use crate::backfill::uid_set;
use regards_core::{attachment, body};
use regards_imap::{ImapError, Session, TextPart};
use regards_store::messages::{self, FileShape};
use std::collections::{BTreeMap, HashMap};

/// How much of a text part is read: enough for a preview once decoded. HTML
/// needs more, to get past its styles.
const PLAIN_BYTES: u32 = 2048;
const HTML_BYTES: u32 = 8192;
/// At most this many messages' details during one sync of a folder.
pub(crate) const AT_SYNC: usize = 200;

/// Fetch and store the details of `uids` in the selected folder; `false` if
/// that failed (logged), so the backfill asks again another time.
pub(crate) async fn fill(state: &State, session: &mut Session, mailbox_id: i64, uids: &[u32]) -> bool {
    if uids.is_empty() {
        return true;
    }
    let found = match fetch(session, uids).await {
        Ok(found) => found,
        Err(error) => {
            tracing::debug!(mailbox = mailbox_id, %error, "no previews from this server for now");
            return false;
        }
    };
    let stored = state
        .store
        .write(move |tx| {
            for (uid, details) in &found {
                messages::set_details(
                    tx,
                    mailbox_id,
                    *uid,
                    details.has_attachments,
                    details.preview.as_deref(),
                    &details.files,
                )?;
            }
            Ok(())
        })
        .await;
    if let Err(error) = stored {
        tracing::warn!(mailbox = mailbox_id, %error, "previews not stored");
        return false;
    }
    true
}

/// Messages whose text is in the same part number: (UID, how to read it).
type Group = Vec<(u32, TextPart)>;

/// What a row shows of a message beyond its headers.
#[derive(Default)]
struct Details {
    has_attachments: bool,
    preview: Option<String>,
    files: Vec<FileShape>,
}

/// For each of `uids`: whether it carries files, and its preview. A message
/// the server says nothing about gets neither, so it is not asked again.
async fn fetch(session: &mut Session, uids: &[u32]) -> Result<HashMap<u32, Details>, ImapError> {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    let shapes = session.fetch_shapes(&uid_set(&sorted)).await?;
    let mut found: HashMap<u32, Details> = sorted.iter().map(|uid| (*uid, Details::default())).collect();
    // One FETCH per part number and kind: most messages share a few.
    let mut groups: BTreeMap<(Vec<u32>, bool), Group> = BTreeMap::new();
    for (uid, shape) in shapes {
        if let Some(entry) = found.get_mut(&uid) {
            entry.has_attachments = shape.has_attachments;
            entry.files = shape
                .files
                .iter()
                .map(|file| FileShape {
                    name: attachment::name_from_parameters(&file.params),
                    mime: file.mime.clone(),
                    // An invitation, or an answer to one (`reply`): the calendar part says which.
                    method: file
                        .params
                        .iter()
                        .find(|(key, _)| key == "method")
                        .map(|(_, value)| value.trim().to_ascii_lowercase()),
                })
                .collect();
        }
        if let Some(text) = shape.text {
            groups
                .entry((text.section.clone(), text.html))
                .or_default()
                .push((uid, text));
        }
    }
    for ((section, html), members) in groups {
        let mut set: Vec<u32> = members.iter().map(|(uid, _)| *uid).collect();
        set.sort_unstable();
        let bytes = if html { HTML_BYTES } else { PLAIN_BYTES };
        let starts = match session.fetch_starts(&uid_set(&set), &section, bytes).await {
            Ok(starts) => starts,
            // A refused part number costs those messages their preview, not the rest.
            Err(ImapError::Protocol(_)) => continue,
            Err(error) => return Err(error),
        };
        let parts: HashMap<u32, &TextPart> = members.iter().map(|(uid, part)| (*uid, part)).collect();
        for (uid, start) in starts {
            let (Some(part), Some(entry)) = (parts.get(&uid), found.get_mut(&uid)) else {
                continue;
            };
            let text = body::preview(&start, &part.encoding, part.charset.as_deref(), part.html);
            entry.preview = (!text.is_empty()).then_some(text);
        }
    }
    Ok(found)
}
