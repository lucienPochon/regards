//! The search language (plan §5.10): Gmail-style, for the keyword index.
//!
//! - Words match anywhere (subject, people, text); the last one also as a
//!   prefix, so results come as one types. `"a phrase"` matches as written,
//!   `-word` leaves out.
//! - `from:` `to:` `subject:` look in one place; `has:attachment`,
//!   `is:unread` / `is:read` / `is:flagged` (`is:starred`), `in:<folder>`
//!   and `before:` / `after:` (`YYYY-MM-DD`, UTC days) filter.
//! - Anything else with a colon is searched as words.
//!
//! What is typed never reaches FTS5's own syntax: every word is quoted.

/// What a search asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// The FTS5 expression for the words, if there are any.
    pub text: Option<String>,
    pub has_attachment: bool,
    /// `Some(true)`: only unread; `Some(false)`: only read.
    pub unread: Option<bool>,
    pub flagged: bool,
    /// A folder, by name or role (`inbox`, `sent` …).
    pub folder: Option<String>,
    /// Received before (exclusive) / on or after, Unix ms.
    pub before: Option<i64>,
    pub after: Option<i64>,
}

impl Query {
    /// Nothing to search for.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

pub fn parse(input: &str) -> Query {
    let mut query = Query::default();
    let mut parts: Vec<String> = Vec::new();
    let tokens = tokens(input);
    let last_word = tokens
        .iter()
        .rposition(|token| matches!(token, Token::Word { negated: false, .. }));
    for (index, token) in tokens.into_iter().enumerate() {
        match token {
            Token::Phrase { text, negated, field } => {
                let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
                if !words.is_empty() {
                    parts.push(term(field, &format!("\"{}\"", escape(&words.join(" "))), negated));
                }
            }
            Token::Word { text, negated, field } => {
                if field.is_none() && !negated && apply(&mut query, &text) {
                    continue;
                }
                // The last word as typed so far: also as the start of a longer
                // one, from two letters (one would match nearly everything).
                let typing = last_word == Some(index) || field.is_some();
                for piece in pieces(&text) {
                    let prefix = if typing && piece.chars().nth(1).is_some() {
                        "*"
                    } else {
                        ""
                    };
                    parts.push(term(field, &format!("\"{}\"{prefix}", escape(&piece)), negated));
                }
            }
        }
    }
    // FTS5 cannot start with NOT: exclusions go after the words, and a query
    // of only exclusions matches nothing.
    let (excluded, wanted): (Vec<String>, Vec<String>) = parts.into_iter().partition(|part| part.starts_with("NOT "));
    if !wanted.is_empty() {
        query.text = Some(wanted.into_iter().chain(excluded).collect::<Vec<_>>().join(" "));
    }
    query
}

/// `has:` `is:` `in:` `before:` `after:`; `false` if `text` is not one.
fn apply(query: &mut Query, text: &str) -> bool {
    let Some((key, value)) = text.split_once(':') else {
        return false;
    };
    let value = value.to_lowercase();
    match (key.to_lowercase().as_str(), value.as_str()) {
        ("has", "attachment" | "attachments" | "file" | "files") => query.has_attachment = true,
        ("is", "unread") => query.unread = Some(true),
        ("is", "read") => query.unread = Some(false),
        ("is", "flagged" | "starred") => query.flagged = true,
        ("in", folder) if !folder.is_empty() => query.folder = Some(folder.to_owned()),
        ("before", day) => match day_start(day) {
            Some(ms) => query.before = Some(ms),
            None => return false,
        },
        ("after", day) => match day_start(day) {
            Some(ms) => query.after = Some(ms),
            None => return false,
        },
        _ => return false,
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Sender,
    Recipients,
    Subject,
}

impl Field {
    fn column(self) -> &'static str {
        match self {
            Self::Sender => "sender",
            Self::Recipients => "recipients",
            Self::Subject => "subject",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word {
        text: String,
        negated: bool,
        field: Option<Field>,
    },
    Phrase {
        text: String,
        negated: bool,
        field: Option<Field>,
    },
}

fn tokens(input: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let negated = c == '-';
        if negated {
            chars.next();
        }
        // A field, then a word or a phrase: from:ada, subject:"the plan".
        let mut word = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == '"' {
                break;
            }
            word.push(c);
            chars.next();
            if c == ':' {
                break;
            }
        }
        let field = word
            .strip_suffix(':')
            .and_then(|key| match key.to_lowercase().as_str() {
                "from" => Some(Field::Sender),
                "to" | "cc" => Some(Field::Recipients),
                "subject" => Some(Field::Subject),
                _ => None,
            });
        if chars.peek() == Some(&'"') && (word.is_empty() || field.is_some()) {
            chars.next();
            let text: String = chars.by_ref().take_while(|c| *c != '"').collect();
            found.push(Token::Phrase { text, negated, field });
            continue;
        }
        // The rest of the word after a field name or a lone colon.
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            word.push(c);
            chars.next();
        }
        let text = match field {
            Some(_) => word
                .split_once(':')
                .map(|(_, rest)| rest.to_owned())
                .unwrap_or_default(),
            None => word,
        };
        if !text.is_empty() {
            found.push(Token::Word { text, negated, field });
        }
    }
    found
}

/// A word as the index splits it: `ada@example.org` is "ada", "example", "org".
fn pieces(word: &str) -> Vec<String> {
    word.split(|c: char| !c.is_alphanumeric())
        .filter(|piece| !piece.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn term(field: Option<Field>, quoted: &str, negated: bool) -> String {
    let scoped = match field {
        Some(field) => format!("{} : {quoted}", field.column()),
        None => quoted.to_owned(),
    };
    if negated { format!("NOT {scoped}") } else { scoped }
}

fn escape(text: &str) -> String {
    text.replace('"', "\"\"")
}

/// Midnight UTC of `YYYY-MM-DD`, Unix ms.
fn day_start(day: &str) -> Option<i64> {
    let mut parts = day.split('-').map(|part| part.parse::<i64>().ok());
    let (Some(Some(year)), Some(Some(month)), Some(Some(date)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&date) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + date - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(input: &str) -> Option<String> {
        parse(input).text
    }

    #[test]
    fn words_phrases_and_exclusions_become_safe_fts5() {
        assert_eq!(text("garden plot"), Some(r#""garden" "plot"*"#.into()));
        assert_eq!(text(r#""seed order" spring"#), Some(r#""seed order" "spring"*"#.into()));
        assert_eq!(text("tickets -lugano"), Some(r#""tickets"* NOT "lugano""#.into()));
        assert_eq!(text("-lugano tickets"), Some(r#""tickets"* NOT "lugano""#.into()));
        // Characters FTS5 would read as syntax are only words here.
        assert_eq!(text(r#"a" OR bc*"#), Some(r#""a" "or" "bc"*"#.into()));
        assert_eq!(
            text("plan b"),
            Some(r#""plan" "b""#.into()),
            "one letter is not a prefix"
        );
        assert_eq!(text("ada@example.org"), Some(r#""ada"* "example"* "org"*"#.into()));
        assert_eq!(text("-only"), None, "exclusions alone match nothing");
        assert_eq!(text("   "), None);
    }

    #[test]
    fn fields_look_in_one_place() {
        assert_eq!(text("from:lena"), Some(r#"sender : "lena"*"#.into()));
        assert_eq!(text(r#"subject:"the plan""#), Some(r#"subject : "the plan""#.into()));
        assert_eq!(
            text("to:bob@example.org"),
            Some(r#"recipients : "bob"* recipients : "example"* recipients : "org"*"#.into())
        );
        assert_eq!(text("plan -from:shop"), Some(r#""plan"* NOT sender : "shop"*"#.into()));
        assert_eq!(text("-from:shop"), None);
    }

    #[test]
    fn filters_are_not_words() {
        let query = parse("has:attachment is:unread is:starred in:Inbox before:2026-10-18 after:2026-10-01 invoice");
        assert!(query.has_attachment && query.flagged);
        assert_eq!(query.unread, Some(true));
        assert_eq!(query.folder.as_deref(), Some("inbox"));
        assert_eq!(query.before, Some(1_792_281_600_000));
        assert_eq!(query.after, Some(1_790_812_800_000));
        assert_eq!(query.text, Some(r#""invoice"*"#.into()));
        // Not a filter after all: searched as words.
        assert_eq!(text("before:soon"), Some(r#""before"* "soon"*"#.into()));
        assert!(parse("is:read").unread == Some(false) && parse("is:read").text.is_none());
        assert!(Query::default().is_empty() && !parse("x").is_empty());
    }

    #[test]
    fn days_are_utc_midnights() {
        assert_eq!(day_start("1970-01-01"), Some(0));
        assert_eq!(day_start("2000-03-01"), Some(951_868_800_000));
        assert_eq!(day_start("2026-13-01"), None);
        assert_eq!(day_start("2026-10"), None);
    }
}
