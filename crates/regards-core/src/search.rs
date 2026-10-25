//! The search language (plan §5.10): Gmail-style, for the keyword index.
//!
//! - Words match anywhere (subject, people, text); the last one also as a
//!   prefix, so results come as one types. `"a phrase"` matches as written,
//!   `-word` leaves out.
//! - `from:` `to:` `subject:` look in one place; `has:attachment`,
//!   `has:pdf` (`image`, `document`, `spreadsheet`, `presentation`,
//!   `archive`), `filename:<part of a name>`,
//!   `is:unread` / `is:read` / `is:flagged` (`is:starred`), `is:pinned`,
//!   `in:<folder>` and `before:` / `after:` (`YYYY-MM-DD`, UTC days)
//!   filter; with a clock ([`parse_at`]), so do "today", "last week" and
//!   the like, in local days.
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
    /// Carrying a file of each of these kinds (`has:pdf`).
    pub file_kinds: Vec<FileKind>,
    /// Carrying a file whose name contains each of these, lowercase (`filename:`).
    pub file_names: Vec<String>,
    /// `Some(true)`: only unread; `Some(false)`: only read.
    pub unread: Option<bool>,
    pub flagged: bool,
    /// Only what is pinned (kept on this device, by Message-ID).
    pub pinned: bool,
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

/// A kind of file, for `has:` (by type or by the name's extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Pdf,
    Image,
    /// Word processing: doc, docx, odt, rtf, pages.
    Document,
    /// xls, xlsx, ods, csv, numbers.
    Spreadsheet,
    /// ppt, pptx, odp, key.
    Presentation,
    /// zip, rar, 7z, tar, gz.
    Archive,
    /// An invitation to an event: a calendar part or an `.ics` file.
    Invitation,
}

impl FileKind {
    /// The types (exactly, or `image/` as a prefix) and name extensions of this kind.
    pub fn types_and_extensions(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::Pdf => (&["application/pdf"], &["pdf"]),
            Self::Image => (
                &["image/"],
                &[
                    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp",
                ],
            ),
            Self::Document => (
                &[
                    "application/msword",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    "application/vnd.oasis.opendocument.text",
                    "application/rtf",
                    "text/rtf",
                    "application/vnd.apple.pages",
                ],
                &["doc", "docx", "odt", "rtf", "pages"],
            ),
            Self::Spreadsheet => (
                &[
                    "application/vnd.ms-excel",
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                    "application/vnd.oasis.opendocument.spreadsheet",
                    "text/csv",
                    "application/vnd.apple.numbers",
                ],
                &["xls", "xlsx", "ods", "csv", "numbers"],
            ),
            Self::Presentation => (
                &[
                    "application/vnd.ms-powerpoint",
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                    "application/vnd.oasis.opendocument.presentation",
                    "application/vnd.apple.keynote",
                ],
                &["ppt", "pptx", "odp", "key"],
            ),
            Self::Archive => (
                &[
                    "application/zip",
                    "application/x-zip-compressed",
                    "application/x-rar-compressed",
                    "application/vnd.rar",
                    "application/x-7z-compressed",
                    "application/x-tar",
                    "application/gzip",
                ],
                &["zip", "rar", "7z", "tar", "gz", "tgz"],
            ),
            Self::Invitation => (&["text/calendar", "application/ics"], &["ics"]),
        }
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
    parse_with(input, None)
}

/// [`parse`], with Spark's relative dates too: `today`, `yesterday`, `this
/// week`, `last week`, `this month`, `last month`, `last 3 days` (or weeks,
/// months; `two` to `ten` spelled out), and the same in French. A week
/// starts on Monday, a day at local midnight: `now` is Unix ms, and
/// `offset_minutes` the local time's offset from UTC.
pub fn parse_at(input: &str, now: i64, offset_minutes: i32) -> Query {
    parse_with(
        input,
        Some(Clock {
            now,
            offset: i64::from(offset_minutes) * 60_000,
        }),
    )
}

fn parse_with(input: &str, clock: Option<Clock>) -> Query {
    let mut query = Query::default();
    let mut parts: Vec<String> = Vec::new();
    let mut tokens = tokens(input);
    if let Some((after, before)) = clock.and_then(|clock| take_relative(&mut tokens, clock)) {
        query.after = Some(after);
        query.before = Some(before);
    }
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
        ("has", kind) => {
            let kind = match kind {
                "pdf" | "pdfs" => FileKind::Pdf,
                "image" | "images" | "photo" | "photos" | "picture" | "pictures" => FileKind::Image,
                "document" | "documents" | "doc" | "docs" => FileKind::Document,
                "spreadsheet" | "spreadsheets" | "sheet" | "sheets" => FileKind::Spreadsheet,
                "presentation" | "presentations" | "slides" => FileKind::Presentation,
                "archive" | "archives" | "zip" | "zips" => FileKind::Archive,
                "invitation" | "invitations" | "invite" | "invites" | "event" | "events" => FileKind::Invitation,
                _ => return false,
            };
            if !query.file_kinds.contains(&kind) {
                query.file_kinds.push(kind);
            }
        }
        ("filename", name) if !name.is_empty() => query.file_names.push(name.to_owned()),
        ("is", "unread") => query.unread = Some(true),
        ("is", "read") => query.unread = Some(false),
        ("is", "flagged" | "starred") => query.flagged = true,
        ("is", "pinned") => query.pinned = true,
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
    Some(days_from_civil(year, month, date) * DAY)
}

const DAY: i64 = 86_400_000;

/// What time it is where the person is, for relative dates.
#[derive(Debug, Clone, Copy)]
struct Clock {
    /// Unix ms.
    now: i64,
    /// Local time minus UTC, in ms.
    offset: i64,
}

/// The first relative date among the plain words (not quoted, no field,
/// not left out), taken out of them: when it starts (inclusive) and ends.
fn take_relative(tokens: &mut Vec<Token>, clock: Clock) -> Option<(i64, i64)> {
    let words: Vec<Option<String>> = tokens
        .iter()
        .map(|token| match token {
            Token::Word {
                text,
                negated: false,
                field: None,
            } => Some(text.to_lowercase()),
            _ => None,
        })
        .collect();
    for start in 0..words.len() {
        for len in (1..=3).rev() {
            let Some(phrase) = words.get(start..start + len) else {
                continue;
            };
            let Some(phrase) = phrase.iter().cloned().collect::<Option<Vec<String>>>() else {
                continue;
            };
            if let Some(range) = relative_range(&phrase, clock) {
                tokens.drain(start..start + len);
                return Some(range);
            }
        }
    }
    None
}

fn relative_range(words: &[String], clock: Clock) -> Option<(i64, i64)> {
    let today = (clock.now + clock.offset).div_euclid(DAY);
    let start = |day: i64| day * DAY - clock.offset;
    let tomorrow = start(today + 1);
    // 1970-01-01 was a Thursday: Monday is 0.
    let monday = today - (today + 3).rem_euclid(7);
    let (year, month, date) = civil_from_days(today);
    let first = days_from_civil(year, month, 1);
    let (last_year, last_month) = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    Some(match words.as_slice() {
        ["today"] | ["aujourd'hui"] | ["aujourd’hui"] => (start(today), tomorrow),
        ["yesterday"] | ["hier"] => (start(today - 1), start(today)),
        ["this", "week"] | ["cette", "semaine"] => (start(monday), tomorrow),
        ["last", "week"] | ["la", "semaine", "dernière"] | ["semaine", "dernière"] => {
            (start(monday - 7), start(monday))
        }
        ["this", "month"] | ["ce", "mois"] | ["ce", "mois-ci"] => (start(first), tomorrow),
        ["last", "month"] | ["le", "mois", "dernier"] | ["mois", "dernier"] => {
            (start(days_from_civil(last_year, last_month, 1)), start(first))
        }
        ["last", count, unit] => {
            let count = number(count)?;
            let from = match unit.trim_end_matches('s') {
                "day" => today + 1 - count,
                "week" => today + 1 - 7 * count,
                // The same date `count` months back (the month's last day if it has fewer).
                "month" => {
                    let back = year * 12 + (month - 1) - count;
                    let (y, m) = (back.div_euclid(12), back.rem_euclid(12) + 1);
                    days_from_civil(y, m, date.min(days_in_month(y, m)))
                }
                _ => return None,
            };
            (start(from), tomorrow)
        }
        _ => return None,
    })
}

/// 1 to 99, or "two" to "ten".
fn number(word: &str) -> Option<i64> {
    const SPELLED: [&str; 9] = ["two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
    match word.parse::<i64>() {
        Ok(n) if (1..=99).contains(&n) => Some(n),
        Ok(_) => None,
        Err(_) => SPELLED.iter().position(|w| *w == word).map(|at| at as i64 + 2),
    }
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, date: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + date - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `days` after 1970-01-01: (year, month, date).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let date = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, date)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    next - days_from_civil(year, month, 1)
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
    fn files_are_filters_by_kind_or_name() {
        let query = parse("has:pdf has:images has:PDF filename:Rapport has:attachment budget");
        assert_eq!(query.file_kinds, [FileKind::Pdf, FileKind::Image], "each kind once");
        assert_eq!(query.file_names, ["rapport"]);
        assert!(query.has_attachment);
        assert_eq!(query.text, Some(r#""budget"*"#.into()));
        assert_eq!(parse("has:sheets has:slides has:zip has:docs").file_kinds.len(), 4);
        assert_eq!(parse("has:invitation").file_kinds, [FileKind::Invitation]);
        assert_eq!(parse("has:invites").file_kinds, [FileKind::Invitation]);
        // Not a kind, nor a name: searched as words.
        assert!(parse("has:cake").file_kinds.is_empty() && parse("has:cake").text.is_some());
        assert!(parse("filename:").file_names.is_empty());
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

    /// Thursday 2026-10-22, 15:00 in Zurich (UTC+2).
    const NOW: i64 = 1_792_674_000_000;
    const ZURICH: i32 = 120;

    fn local(day: &str) -> i64 {
        day_start(day).unwrap() - 120 * 60_000
    }

    fn dates(input: &str) -> (Option<i64>, Option<i64>, Option<String>) {
        let query = parse_at(input, NOW, ZURICH);
        (query.after, query.before, query.text)
    }

    #[test]
    fn relative_dates_are_local_days_and_leave_the_other_words() {
        assert_eq!(
            dates("today"),
            (Some(local("2026-10-22")), Some(local("2026-10-23")), None)
        );
        assert_eq!(
            dates("Yesterday"),
            (Some(local("2026-10-21")), Some(local("2026-10-22")), None)
        );
        // Weeks start on Monday.
        assert_eq!(dates("this week").0, Some(local("2026-10-19")));
        assert_eq!(
            dates("report last week"),
            (
                Some(local("2026-10-12")),
                Some(local("2026-10-19")),
                Some("\"report\"*".into())
            )
        );
        assert_eq!(
            dates("last month"),
            (Some(local("2026-09-01")), Some(local("2026-10-01")), None)
        );
        assert_eq!(dates("this month").0, Some(local("2026-10-01")));
        assert_eq!(dates("last 3 days").0, Some(local("2026-10-20")));
        assert_eq!(dates("last two weeks").0, Some(local("2026-10-09")));
        assert_eq!(dates("last 2 months").0, Some(local("2026-08-22")));
        // In French too.
        assert_eq!(dates("hier").0, Some(local("2026-10-21")));
        assert_eq!(dates("la semaine dernière").0, Some(local("2026-10-12")));
        assert_eq!(dates("le mois dernier").0, Some(local("2026-09-01")));
        // Quoted, or without a clock, they are words.
        assert_eq!(dates("\"last week\"").0, None);
        assert_eq!(parse("today").after, None);
        assert_eq!(dates("last summer").0, None);
    }

    #[test]
    fn the_calendar_helpers_agree() {
        for day in [-1, 0, 59, 10_957, 20_383, 20_747] {
            let (y, m, d) = civil_from_days(day);
            assert_eq!(days_from_civil(y, m, d), day);
        }
        assert_eq!(
            (days_in_month(2024, 2), days_in_month(2026, 2), days_in_month(2026, 12)),
            (29, 28, 31)
        );
        assert!(parse("is:pinned budget").pinned);
    }
}
