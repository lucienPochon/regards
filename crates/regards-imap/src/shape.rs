//! What a message's structure says, for its row in a list (BODYSTRUCTURE):
//! whether it carries files (and which: their types and names, for search),
//! and which part holds the start of its text.
//!
//! Fetched apart from the headers: servers that send a malformed structure
//! then cost a message its preview, not its place in the folder.

use crate::conn::Reply;
use crate::{ImapError, Session};
use imap_types::body::{BodyStructure, SpecificFields};
use imap_types::command::CommandBody;
use imap_types::core::Vec1;
use imap_types::fetch::{MessageDataItem, MessageDataItemName, Part, Section};
use imap_types::response::Data;
use std::num::NonZeroU32;

/// A message's shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shape {
    /// It carries files (attached, not shown inline in its text).
    pub has_attachments: bool,
    /// Where its text is: a plain-text part if there is one, else HTML.
    pub text: Option<TextPart>,
    /// The files, in order.
    pub files: Vec<FilePart>,
}

/// A file a message carries, as its structure describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePart {
    /// `type/subtype`, lowercase.
    pub mime: String,
    /// Its Content-Type and Content-Disposition parameters as sent (keys
    /// lowercase): the name may be split or encoded (RFC 2231, RFC 2047),
    /// which the reader of these decodes.
    pub params: Vec<(String, String)>,
}

/// The part holding a message's text, and how to read it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextPart {
    /// Its part number, for example `[1]` or `[1, 2]`.
    pub section: Vec<u32>,
    pub html: bool,
    /// `Content-Transfer-Encoding`, lowercase: "base64", "quoted-printable", "7bit" …
    pub encoding: String,
    pub charset: Option<String>,
}

impl Session {
    /// The shape of each message in `uids` (a UID set).
    pub async fn fetch_shapes(&mut self, uids: &str) -> Result<Vec<(u32, Shape)>, ImapError> {
        let reply = self
            .fetch(uids, vec![MessageDataItemName::Uid, MessageDataItemName::BodyStructure])
            .await?;
        let mut shapes = Vec::new();
        for data in &reply.data {
            let Data::Fetch { items, .. } = data else { continue };
            let mut uid = 0;
            let mut shape = None;
            for item in items.as_ref() {
                match item {
                    MessageDataItem::Uid(found) => uid = found.get(),
                    MessageDataItem::BodyStructure(structure) => shape = Some(shape_of(structure)),
                    _ => {}
                }
            }
            if let (true, Some(shape)) = (uid != 0, shape) {
                shapes.push((uid, shape));
            }
        }
        Ok(shapes)
    }

    /// The first `bytes` octets of part `section` of each message in `uids`,
    /// as stored (still transfer-encoded), without marking them read.
    pub async fn fetch_starts(
        &mut self,
        uids: &str,
        section: &[u32],
        bytes: u32,
    ) -> Result<Vec<(u32, Vec<u8>)>, ImapError> {
        let parts: Vec<NonZeroU32> = section.iter().filter_map(|n| NonZeroU32::new(*n)).collect();
        let part = Vec1::try_from(parts).map_err(|_| ImapError::Protocol("an empty part number".into()))?;
        let length = NonZeroU32::new(bytes).ok_or_else(|| ImapError::Protocol("a zero-length fetch".into()))?;
        let reply = self
            .fetch(
                uids,
                vec![
                    MessageDataItemName::Uid,
                    MessageDataItemName::BodyExt {
                        section: Some(Section::Part(Part(part))),
                        partial: Some((0, length)),
                        peek: true,
                    },
                ],
            )
            .await?;
        let mut starts = Vec::new();
        for data in &reply.data {
            let Data::Fetch { items, .. } = data else { continue };
            let mut uid = 0;
            let mut start = None;
            for item in items.as_ref() {
                match item {
                    MessageDataItem::Uid(found) => uid = found.get(),
                    MessageDataItem::BodyExt { data, .. } => {
                        start = Some(data.0.as_ref().map(|s| s.as_ref().to_vec()).unwrap_or_default());
                    }
                    _ => {}
                }
            }
            if let (true, Some(start)) = (uid != 0, start) {
                starts.push((uid, start));
            }
        }
        Ok(starts)
    }

    async fn fetch(&mut self, uids: &str, items: Vec<MessageDataItemName<'static>>) -> Result<Reply, ImapError> {
        let reply = self
            .conn
            .run_skipping(CommandBody::Fetch {
                sequence_set: crate::messages::sequence_set(uids)?,
                macro_or_item_names: items.into(),
                uid: true,
                modifiers: vec![],
            })
            .await?;
        crate::messages::ok(&reply, "FETCH")?;
        Ok(reply)
    }
}

/// Walk a structure: the first plain-text part (else the first HTML one)
/// that is not a file, and whether any part is a file.
pub fn shape_of(structure: &BodyStructure<'_>) -> Shape {
    let mut shape = Shape::default();
    let mut html = None;
    walk(structure, &mut Vec::new(), false, &mut shape, &mut html);
    if shape.text.is_none() {
        shape.text = html;
    }
    shape
}

fn walk(
    structure: &BodyStructure<'_>,
    path: &mut Vec<u32>,
    related: bool,
    shape: &mut Shape,
    html: &mut Option<TextPart>,
) {
    match structure {
        BodyStructure::Multi { bodies, subtype, .. } => {
            let related = related || lower(subtype.as_ref()) == "related";
            for (index, body) in bodies.as_ref().iter().enumerate() {
                path.push(index as u32 + 1);
                walk(body, path, related, shape, html);
                path.pop();
            }
        }
        BodyStructure::Single { body, extension_data } => {
            let disposition = extension_data
                .as_ref()
                .and_then(|data| data.tail.as_ref())
                .and_then(|tail| tail.disposition.as_ref());
            let attached = disposition.is_some_and(|(kind, _)| lower(kind.as_ref()) == "attachment");
            let named = disposition.is_some_and(|(_, params)| has(params, "filename"))
                || has(&body.basic.parameter_list, "name");
            // A part number for a single-part message is 1.
            let section = if path.is_empty() { vec![1] } else { path.clone() };
            match &body.specific {
                SpecificFields::Text { subtype, .. } if !attached => {
                    let subtype = lower(subtype.as_ref());
                    // An invitation: listed with its files, to find it by, though it shows no paperclip.
                    if subtype == "calendar" {
                        shape.files.push(FilePart {
                            mime: "text/calendar".to_owned(),
                            params: pairs(&body.basic.parameter_list),
                        });
                        return;
                    }
                    let part = || TextPart {
                        section: section.clone(),
                        html: subtype == "html",
                        encoding: lower(body.basic.content_transfer_encoding.as_ref()),
                        charset: param(&body.basic.parameter_list, "charset"),
                    };
                    if subtype == "plain" && shape.text.is_none() {
                        shape.text = Some(part());
                    } else if subtype == "html" && html.is_none() {
                        *html = Some(part());
                    }
                }
                // Images shown in the text (multipart/related) are not files to list.
                _ if attached || (named && !related) => {
                    shape.has_attachments = true;
                    let mut params = pairs(&body.basic.parameter_list);
                    if let Some((_, disposition)) = disposition {
                        params.extend(pairs(disposition));
                    }
                    shape.files.push(FilePart {
                        mime: mime_of(&body.specific),
                        params,
                    });
                }
                _ => {}
            }
        }
    }
}

fn lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

fn param(params: &[(imap_types::core::IString<'_>, imap_types::core::IString<'_>)], name: &str) -> Option<String> {
    params
        .iter()
        .find(|(key, _)| lower(key.as_ref()) == name)
        .map(|(_, value)| String::from_utf8_lossy(value.as_ref()).trim().to_owned())
}

/// A single part's `type/subtype`, lowercase.
fn mime_of(specific: &SpecificFields<'_>) -> String {
    match specific {
        SpecificFields::Basic { r#type, subtype } => format!("{}/{}", lower(r#type.as_ref()), lower(subtype.as_ref())),
        SpecificFields::Message { .. } => "message/rfc822".into(),
        SpecificFields::Text { subtype, .. } => format!("text/{}", lower(subtype.as_ref())),
    }
}

fn pairs(params: &[(imap_types::core::IString<'_>, imap_types::core::IString<'_>)]) -> Vec<(String, String)> {
    params
        .iter()
        .map(|(key, value)| {
            (
                lower(key.as_ref()),
                String::from_utf8_lossy(value.as_ref()).into_owned(),
            )
        })
        .collect()
}

fn has(params: &[(imap_types::core::IString<'_>, imap_types::core::IString<'_>)], name: &str) -> bool {
    params.iter().any(|(key, _)| lower(key.as_ref()).starts_with(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use imap_codec::ResponseCodec;
    use imap_codec::decode::Decoder;
    use imap_types::response::Response;

    fn shape(structure: &str) -> Shape {
        let line = format!("* 1 FETCH (UID 7 BODYSTRUCTURE {structure})\r\n");
        let (_, response) = ResponseCodec::default()
            .decode(line.as_bytes())
            .unwrap_or_else(|error| panic!("{structure}: {error:?}"));
        let Response::Data(Data::Fetch { items, .. }) = response else {
            panic!("not a FETCH");
        };
        items
            .as_ref()
            .iter()
            .find_map(|item| match item {
                MessageDataItem::BodyStructure(structure) => Some(shape_of(structure)),
                _ => None,
            })
            .expect("a body structure")
    }

    const PLAIN: &str = r#"("TEXT" "PLAIN" ("CHARSET" "UTF-8") NIL NIL "QUOTED-PRINTABLE" 120 4 NIL NIL NIL NIL)"#;
    const HTML: &str = r#"("TEXT" "HTML" ("CHARSET" "iso-8859-1") NIL NIL "BASE64" 200 3 NIL NIL NIL NIL)"#;

    #[test]
    fn a_plain_message_is_its_own_text() {
        let found = shape(PLAIN);
        assert!(!found.has_attachments);
        let text = found.text.unwrap();
        assert_eq!(text.section, [1]);
        assert!(!text.html);
        assert_eq!(
            (text.encoding.as_str(), text.charset.as_deref()),
            ("quoted-printable", Some("UTF-8"))
        );
    }

    #[test]
    fn plain_text_is_preferred_and_files_are_noticed() {
        let alternative = format!(r#"({PLAIN}{HTML} "ALTERNATIVE" ("BOUNDARY" "a") NIL NIL NIL)"#);
        assert_eq!(shape(&alternative).text.unwrap().section, [1]);
        let pdf = r#"("APPLICATION" "PDF" ("NAME" "Ticket.pdf") NIL NIL "BASE64" 1000 NIL ("ATTACHMENT" ("FILENAME" "Ticket.pdf")) NIL NIL)"#;
        let mixed = format!(r#"({alternative}{pdf} "MIXED" ("BOUNDARY" "m") NIL NIL NIL)"#);
        let found = shape(&mixed);
        assert!(found.has_attachments);
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.files[0].mime, "application/pdf");
        assert_eq!(
            found.files[0].params,
            [
                ("name".to_owned(), "Ticket.pdf".to_owned()),
                ("filename".to_owned(), "Ticket.pdf".to_owned())
            ]
        );
        assert_eq!(found.text.unwrap().section, [1, 1]);
    }

    #[test]
    fn html_alone_is_the_text_and_its_inline_images_are_not_files() {
        let logo = r#"("IMAGE" "PNG" ("NAME" "logo.png") "<logo@x>" NIL "BASE64" 500 NIL ("INLINE" ("FILENAME" "logo.png")) NIL NIL)"#;
        let related = format!(r#"({HTML}{logo} "RELATED" ("BOUNDARY" "r") NIL NIL NIL)"#);
        let found = shape(&related);
        assert!(!found.has_attachments, "the logo is part of the text");
        let text = found.text.unwrap();
        assert_eq!(text.section, [1]);
        assert!(text.html);
        assert_eq!(text.charset.as_deref(), Some("iso-8859-1"));
    }

    #[test]
    fn an_invitation_is_listed_with_the_files_without_a_paperclip() {
        let calendar =
            r#"("TEXT" "CALENDAR" ("CHARSET" "UTF-8" "METHOD" "REQUEST") NIL NIL "7BIT" 900 20 NIL NIL NIL NIL)"#;
        let alternative = format!(r#"({PLAIN}{HTML}{calendar} "ALTERNATIVE" ("BOUNDARY" "a") NIL NIL NIL)"#);
        let found = shape(&alternative);
        assert!(!found.has_attachments, "an invitation is not a file to save");
        assert_eq!(found.files.len(), 1);
        assert_eq!(found.files[0].mime, "text/calendar");
        assert!(
            found.files[0]
                .params
                .contains(&("method".to_owned(), "REQUEST".to_owned()))
        );
        assert_eq!(found.text.unwrap().section, [1], "the text is still the plain one");
    }
}
