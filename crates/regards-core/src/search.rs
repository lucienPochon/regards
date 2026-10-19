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
    /// With no words wanted, only some left out (`-newsletter`): what to leave out.
    pub excluded: Option<String>,
    /// The words and phrases as typed, for searching a server.
    pub terms: Vec<Term>,
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

/// A word or phrase of a search, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub text: String,
    /// Where to look; anywhere when `None`.
    pub field: Option<Field>,
    pub negated: bool,
    /// Also matched as the start of a longer word: the word being typed,
    /// or one with a field.
    pub prefix: bool,
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
                    query.terms.push(Term {
                        text: words.join(" "),
                        field,
                        negated,
                        prefix: false,
                    });
                }
            }
            Token::Word { text, negated, field } => {
                if field.is_none() && !negated && apply(&mut query, &text) {
                    continue;
                }
                if pieces(&text).is_empty() {
                    continue;
                }
                // The last word as typed so far: also as the start of a longer
                // one, from two letters (one would match nearly everything).
                let typing = last_word == Some(index) || field.is_some();
                query.terms.push(Term {
                    text: text.clone(),
                    field,
                    negated,
                    prefix: typing && !negated,
                });
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
    // FTS5 cannot start with NOT: exclusions go after the words. With only
    // exclusions, what matches any of them is left out of everything.
    let (excluded, wanted): (Vec<String>, Vec<String>) = parts.into_iter().partition(|part| part.starts_with("NOT "));
    if !wanted.is_empty() {
        query.text = Some(wanted.into_iter().chain(excluded).collect::<Vec<_>>().join(" "));
    } else if !excluded.is_empty() {
        let any: Vec<&str> = excluded.iter().map(|part| &part["NOT ".len()..]).collect();
        query.excluded = Some(any.join(" OR "));
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

/// Where `from:` `to:` `subject:` look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Sender,
    /// To or Cc.
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

/// A word as the index keeps it: lower case, without the accents of Latin
/// letters (the index's `remove_diacritics`, for those that have a base
/// letter; others are kept, as there).
pub fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'ď' => 'd',
            'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
            'ĥ' => 'h',
            'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
            'ĵ' => 'j',
            'ķ' => 'k',
            'ĺ' | 'ļ' | 'ľ' => 'l',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'ò'..='ö' | 'ō' | 'ŏ' | 'ő' => 'o',
            'ŕ' | 'ŗ' | 'ř' => 'r',
            'ś' | 'ŝ' | 'ş' | 'š' => 's',
            'ţ' | 'ť' => 't',
            'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'ŵ' => 'w',
            'ý' | 'ÿ' | 'ŷ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            other => other,
        })
        .collect()
}

/// How many edits make `a` into `b` (a letter wrong, missing, extra, or
/// two side by side swapped), if at most `max`.
pub fn edits_within(a: &str, b: &str, max: usize) -> Option<usize> {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    // Rows of the optimal string alignment distance; the one before last for swaps.
    let mut before: Vec<usize> = Vec::new();
    let mut last: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut row = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            row[j] = (last[j] + 1).min(row[j - 1] + 1).min(last[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                row[j] = row[j].min(before[j - 2] + 1);
            }
        }
        if row.iter().min().is_some_and(|&least| least > max) {
            return None;
        }
        before = std::mem::replace(&mut last, row);
    }
    Some(last[b.len()]).filter(|&edits| edits <= max)
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
        assert_eq!(text("-only"), None);
        assert_eq!(
            parse("-only -from:shop").excluded,
            Some(r#""only" OR sender : "shop"*"#.into()),
            "left out"
        );
        assert_eq!(parse("plan -only").excluded, None, "left out of the words");
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
    fn terms_are_kept_as_typed_for_a_server() {
        let query = parse(r#"ada@example.org "seed order" -from:shop subject:Plan is:unread"#);
        let term = |text: &str, field, negated, prefix| Term {
            text: text.into(),
            field,
            negated,
            prefix,
        };
        assert_eq!(
            query.terms,
            [
                term("ada@example.org", None, false, false),
                term("seed order", None, false, false),
                term("shop", Some(Field::Sender), true, false),
                term("Plan", Some(Field::Subject), false, true),
            ]
        );
        assert!(parse("*** \"\"").terms.is_empty(), "nothing to search for");
    }

    #[test]
    fn words_fold_and_typos_are_counted() {
        assert_eq!(fold("Müller"), "muller");
        assert_eq!(fold("CAFÉ"), "cafe");
        assert_eq!(fold("Øre"), "øre", "no base letter, as in the index");
        assert_eq!(edits_within("project", "project", 2), Some(0));
        assert_eq!(edits_within("projcet", "project", 1), Some(1), "two letters swapped");
        assert_eq!(edits_within("proect", "project", 1), Some(1), "one missing");
        assert_eq!(edits_within("prooject", "project", 1), Some(1), "one extra");
        assert_eq!(edits_within("prjoetc", "project", 2), Some(2));
        assert_eq!(edits_within("budget", "project", 2), None);
        assert_eq!(edits_within("a", "abcd", 2), None, "too different in length");
    }

    #[test]
    fn days_are_utc_midnights() {
        assert_eq!(day_start("1970-01-01"), Some(0));
        assert_eq!(day_start("2000-03-01"), Some(951_868_800_000));
        assert_eq!(day_start("2026-13-01"), None);
        assert_eq!(day_start("2026-10"), None);
    }
}
