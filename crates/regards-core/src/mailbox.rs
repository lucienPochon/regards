//! Mailbox roles: which folder is the inbox, sent mail, drafts and so on.
//!
//! Servers that support SPECIAL-USE (RFC 6154) label these folders. Many do
//! not, so common folder names are recognised too. Each account gets at most
//! one mailbox per role: server labels first, then names, first match wins.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    /// Every message (Gmail's "All Mail").
    All,
    /// Flagged messages, as a virtual folder.
    Flagged,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::All => "all",
            Self::Flagged => "flagged",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Inbox,
            Self::Sent,
            Self::Drafts,
            Self::Trash,
            Self::Junk,
            Self::Archive,
            Self::All,
            Self::Flagged,
        ]
        .into_iter()
        .find(|role| role.as_str() == value)
    }

    /// The RFC 6154 attribute for this role, without the backslash.
    fn special_use(self) -> Option<&'static str> {
        match self {
            Self::Inbox => None,
            Self::Sent => Some("sent"),
            Self::Drafts => Some("drafts"),
            Self::Trash => Some("trash"),
            Self::Junk => Some("junk"),
            Self::Archive => Some("archive"),
            Self::All => Some("all"),
            Self::Flagged => Some("flagged"),
        }
    }

    /// Common folder names for this role, lower-case.
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Inbox => &[],
            Self::Sent => &[
                "sent",
                "sent items",
                "sent messages",
                "sent mail",
                "envoyés",
                "éléments envoyés",
                "messages envoyés",
                "gesendet",
                "gesendete elemente",
                "gesendete objekte",
                "enviados",
                "elementos enviados",
                "inviati",
                "posta inviata",
            ],
            Self::Drafts => &["drafts", "draft", "brouillons", "entwürfe", "borradores", "bozze"],
            Self::Trash => &[
                "trash",
                "deleted",
                "deleted items",
                "deleted messages",
                "bin",
                "corbeille",
                "éléments supprimés",
                "papierkorb",
                "gelöschte elemente",
                "papelera",
                "elementos eliminados",
                "cestino",
                "posta eliminata",
            ],
            Self::Junk => &[
                "junk",
                "spam",
                "junk e-mail",
                "junk email",
                "junk mail",
                "courrier indésirable",
                "pourriel",
                "indésirables",
                "junk-e-mail",
                "correo no deseado",
                "posta indesiderata",
            ],
            Self::Archive => &["archive", "archives", "archiv", "archivio", "archivo"],
            Self::All | Self::Flagged => &[],
        }
    }
}

/// What the server reported about one mailbox.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    /// Decoded name, components separated by `delimiter`.
    pub name: &'a str,
    pub delimiter: Option<char>,
    /// LIST attributes such as `\Sent` or `\Noselect`, as the server sent them.
    pub attributes: &'a [String],
}

impl Candidate<'_> {
    fn has_attribute(&self, name: &str) -> bool {
        self.attributes
            .iter()
            .any(|attribute| attribute.trim_start_matches('\\').eq_ignore_ascii_case(name))
    }

    pub fn is_selectable(&self) -> bool {
        !self.has_attribute("noselect") && !self.has_attribute("nonexistent")
    }

    /// The folder's own name when it sits at the top level, under INBOX, or
    /// under Gmail's system folder: the places providers put special folders.
    fn special_folder_name(&self) -> Option<String> {
        let parts: Vec<&str> = match self.delimiter {
            Some(delimiter) => self.name.split(delimiter).collect(),
            None => vec![self.name],
        };
        let leaf = match parts.as_slice() {
            [leaf] => leaf,
            [parent, leaf]
                if parent.eq_ignore_ascii_case("inbox")
                    || parent.eq_ignore_ascii_case("[gmail]")
                    || parent.eq_ignore_ascii_case("[google mail]") =>
            {
                leaf
            }
            _ => return None,
        };
        Some(leaf.to_lowercase())
    }
}

/// The role of each mailbox, in input order.
pub fn assign_roles(mailboxes: &[Candidate<'_>]) -> Vec<Option<Role>> {
    let mut roles: Vec<Option<Role>> = vec![None; mailboxes.len()];
    let mut taken = Vec::new();
    let mut claim = |index: usize, role: Role, roles: &mut Vec<Option<Role>>| {
        if roles[index].is_none() && !taken.contains(&role) {
            roles[index] = Some(role);
            taken.push(role);
        }
    };

    if let Some(inbox) = mailboxes.iter().position(|m| m.name.eq_ignore_ascii_case("inbox")) {
        claim(inbox, Role::Inbox, &mut roles);
    }
    let others = [
        Role::Sent,
        Role::Drafts,
        Role::Trash,
        Role::Junk,
        Role::Archive,
        Role::All,
        Role::Flagged,
    ];
    for (index, mailbox) in mailboxes.iter().enumerate() {
        if !mailbox.is_selectable() {
            continue;
        }
        for role in others {
            if role
                .special_use()
                .is_some_and(|attribute| mailbox.has_attribute(attribute))
            {
                claim(index, role, &mut roles);
            }
        }
    }
    for (index, mailbox) in mailboxes.iter().enumerate() {
        if !mailbox.is_selectable() {
            continue;
        }
        let Some(leaf) = mailbox.special_folder_name() else {
            continue;
        };
        if let Some(role) = others.into_iter().find(|role| role.names().contains(&leaf.as_str())) {
            claim(index, role, &mut roles);
        }
    }
    roles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assign(mailboxes: &[(&str, &[&str])]) -> Vec<Option<Role>> {
        let attributes: Vec<Vec<String>> = mailboxes
            .iter()
            .map(|(_, attrs)| attrs.iter().map(|a| a.to_string()).collect())
            .collect();
        let candidates: Vec<Candidate<'_>> = mailboxes
            .iter()
            .zip(&attributes)
            .map(|((name, _), attributes)| Candidate {
                name,
                delimiter: Some('/'),
                attributes,
            })
            .collect();
        assign_roles(&candidates)
    }

    #[test]
    fn special_use_attributes_win_over_names() {
        let roles = assign(&[
            ("INBOX", &[]),
            ("Sent", &[]),
            ("[Gmail]/Sent Mail", &["\\Sent", "\\HasNoChildren"]),
            ("[Gmail]/All Mail", &["\\All"]),
            ("[Gmail]", &["\\Noselect"]),
        ]);
        assert_eq!(
            roles,
            [Some(Role::Inbox), None, Some(Role::Sent), Some(Role::All), None]
        );
    }

    #[test]
    fn names_are_recognised_without_special_use_in_several_languages() {
        let roles = assign(&[
            ("inbox", &[]),
            ("Envoyés", &[]),
            ("Brouillons", &[]),
            ("Corbeille", &[]),
            ("Spam", &[]),
            ("Projects/Sent", &[]),
        ]);
        assert_eq!(
            roles,
            [
                Some(Role::Inbox),
                Some(Role::Sent),
                Some(Role::Drafts),
                Some(Role::Trash),
                Some(Role::Junk),
                None // a folder named Sent deep in a project is just a folder
            ]
        );
    }

    #[test]
    fn folders_under_inbox_count_and_each_role_is_used_once() {
        let roles = assign(&[
            ("INBOX", &[]),
            ("INBOX/Sent", &[]),
            ("INBOX/Sent Items", &[]),
            ("Trash", &["\\Trash"]),
            ("Deleted Items", &[]),
        ]);
        assert_eq!(
            roles,
            [Some(Role::Inbox), Some(Role::Sent), None, Some(Role::Trash), None]
        );
    }

    #[test]
    fn unselectable_folders_get_no_role() {
        let roles = assign(&[("Archive", &["\\Noselect"]), ("Archives", &[])]);
        assert_eq!(roles, [None, Some(Role::Archive)]);
    }

    #[test]
    fn roles_round_trip_as_strings() {
        for role in [Role::Inbox, Role::Sent, Role::Junk, Role::Flagged] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("nope"), None);
    }
}
