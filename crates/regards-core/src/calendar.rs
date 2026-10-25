//! Invitations by email (iMIP, RFC 6047): the event an iCalendar part
//! (RFC 5545) asks the person to, and the reply that answers it (RFC 5546
//! REPLY). Only what an invitation card shows and a reply needs is read:
//! the first event, its organizer and attendees, when and where.

use crate::headers::Person;

/// An invitation (or its cancellation, or a reply to one): its first event.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Invitation {
    /// `REQUEST` (an invitation), `CANCEL`, `REPLY`… upper case; empty when unsaid.
    pub method: String,
    pub uid: String,
    pub sequence: u32,
    pub summary: String,
    pub location: Option<String>,
    pub start: Option<When>,
    pub end: Option<When>,
    pub organizer: Option<Person>,
    pub attendees: Vec<Attendee>,
    /// It repeats (an `RRULE`): the reply answers every occurrence.
    pub repeats: bool,
    /// One occurrence of a repeating event, as the invitation names it.
    recurrence_id: Option<String>,
    /// The start as the invitation writes it, for the reply.
    start_line: Option<String>,
}

/// A time as an invitation writes it. The app turns it into local time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct When {
    /// `20261030T100000`, or `20261030` for a whole day.
    pub value: String,
    /// The zone it is in (`Europe/Zurich`, or a Windows name), when it is not UTC.
    pub zone: Option<String>,
    /// In UTC (`…Z`).
    pub utc: bool,
    /// A whole day, not a time.
    pub all_day: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub person: Person,
    /// `ACCEPTED`, `DECLINED`, `TENTATIVE`, `NEEDS-ACTION`… upper case.
    pub status: Option<String>,
}

/// How the person answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Accepted,
    Tentative,
    Declined,
}

impl Answer {
    fn partstat(self) -> &'static str {
        match self {
            Self::Accepted => "ACCEPTED",
            Self::Tentative => "TENTATIVE",
            Self::Declined => "DECLINED",
        }
    }

    /// The reply's subject, before the event's: "Accepted: Planning".
    pub fn word(self) -> &'static str {
        match self {
            Self::Accepted => "Accepted",
            Self::Tentative => "Tentatively accepted",
            Self::Declined => "Declined",
        }
    }
}

/// One content line: its name, parameters and value (still escaped).
struct Line<'a> {
    name: String,
    params: Vec<(String, String)>,
    value: &'a str,
}

impl Line<'_> {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Lines folded onto the next (a CRLF then a space or tab) joined again.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match (raw.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(raw.to_owned()),
        }
    }
    lines
}

/// `NAME;PARAM=VALUE;PARAM="quoted":value`, split at the first colon outside quotes.
fn line(text: &str) -> Option<Line<'_>> {
    let mut quoted = false;
    let colon = text.char_indices().find_map(|(at, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        (c == ':' && !quoted).then_some(at)
    })?;
    let (head, value) = (&text[..colon], &text[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| {
            (
                key.trim().to_ascii_uppercase(),
                value.trim().trim_matches('"').to_owned(),
            )
        })
        .collect();
    Some(Line { name, params, value })
}

/// A TEXT value unescaped: `\n`, `\,`, `\;`, `\\`.
fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A person named by a `mailto:` address, with its `CN`.
fn person(line: &Line<'_>) -> Option<Person> {
    let value = line.value.trim();
    let address = value
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        .map_or(value, |_| &value[7..]);
    if !address.contains('@') {
        return None;
    }
    Some(Person {
        name: line.param("CN").map(str::to_owned).filter(|name| !name.is_empty()),
        address: Some(address.to_owned()),
    })
}

fn when(line: &Line<'_>) -> Option<When> {
    let value = line.value.trim();
    let all_day = line
        .param("VALUE")
        .is_some_and(|kind| kind.eq_ignore_ascii_case("DATE"))
        || value.len() == 8;
    let digits = value.trim_end_matches('Z');
    let well_formed = match all_day {
        true => digits.len() == 8 && digits.bytes().all(|b| b.is_ascii_digit()),
        false => digits.len() == 15 && digits.as_bytes()[8] == b'T',
    };
    well_formed.then(|| When {
        value: digits.to_owned(),
        zone: line.param("TZID").map(str::to_owned),
        utc: value.ends_with('Z'),
        all_day,
    })
}

/// The iCalendar text a message carries: its `text/calendar` part, or else
/// an `.ics` file (`application/ics`); `None` without either.
pub fn in_message(source: &[u8]) -> Option<String> {
    use mail_parser::{MessageParser, MimeHeaders, PartType};
    let message = MessageParser::default().parse(source)?;
    let of_kind = |main: &str, sub: &str| {
        message.parts.iter().find_map(|part| {
            let kind = part.content_type()?;
            let matches =
                kind.ctype().eq_ignore_ascii_case(main) && kind.subtype().is_some_and(|s| s.eq_ignore_ascii_case(sub));
            matches.then(|| match &part.body {
                PartType::Text(text) => Some(text.to_string()),
                PartType::Binary(bytes) | PartType::InlineBinary(bytes) => {
                    Some(String::from_utf8_lossy(bytes).into_owned())
                }
                _ => None,
            })?
        })
    };
    of_kind("text", "calendar").or_else(|| of_kind("application", "ics"))
}

/// The first event of an iCalendar text, or `None` if it holds none.
pub fn parse(ics: &str) -> Option<Invitation> {
    let mut invitation = Invitation::default();
    let (mut depth, mut in_event, mut done) = (Vec::<String>::new(), false, false);
    for raw in unfold(ics) {
        let Some(line) = line(&raw) else { continue };
        match line.name.as_str() {
            "BEGIN" => {
                let kind = line.value.trim().to_ascii_uppercase();
                if kind == "VEVENT" && !done && depth.last().is_some_and(|parent| parent == "VCALENDAR") {
                    in_event = true;
                }
                depth.push(kind);
                continue;
            }
            "END" => {
                if depth.pop().as_deref() == Some("VEVENT") && in_event {
                    in_event = false;
                    done = true;
                }
                continue;
            }
            _ => {}
        }
        let at_calendar = depth.last().is_some_and(|parent| parent == "VCALENDAR");
        // The event's own properties, not those of its alarms.
        let at_event = in_event && depth.last().is_some_and(|parent| parent == "VEVENT");
        match line.name.as_str() {
            "METHOD" if at_calendar => invitation.method = line.value.trim().to_ascii_uppercase(),
            "UID" if at_event => invitation.uid = line.value.trim().to_owned(),
            "SEQUENCE" if at_event => invitation.sequence = line.value.trim().parse().unwrap_or(0),
            "SUMMARY" if at_event => invitation.summary = text(line.value),
            "LOCATION" if at_event => {
                invitation.location = Some(text(line.value)).filter(|place| !place.trim().is_empty())
            }
            "DTSTART" if at_event => {
                invitation.start = when(&line);
                invitation.start_line = Some(raw.clone());
            }
            "DTEND" if at_event => invitation.end = when(&line),
            "RRULE" if at_event => invitation.repeats = true,
            "RECURRENCE-ID" if at_event => invitation.recurrence_id = Some(raw.clone()),
            "ORGANIZER" if at_event => invitation.organizer = person(&line),
            "ATTENDEE" if at_event => {
                if let Some(person) = person(&line) {
                    invitation.attendees.push(Attendee {
                        person,
                        status: line.param("PARTSTAT").map(str::to_ascii_uppercase),
                    });
                }
            }
            _ => {}
        }
    }
    (done && !invitation.uid.is_empty()).then_some(invitation)
}

impl Invitation {
    /// How `address` (any case) stands in it, as an attendee, if they are one.
    pub fn status_of(&self, address: &str) -> Option<&str> {
        self.attendees
            .iter()
            .find(|attendee| {
                attendee
                    .person
                    .address
                    .as_deref()
                    .is_some_and(|own| own.eq_ignore_ascii_case(address))
            })
            .and_then(|attendee| attendee.status.as_deref())
    }

    /// The reply to send the organizer (RFC 5546 REPLY): `me` answers.
    /// `now` is Unix milliseconds.
    pub fn reply(&self, me: &Person, answer: Answer, now: i64) -> String {
        let escape = |value: &str| {
            value
                .replace('\\', "\\\\")
                .replace(';', "\\;")
                .replace(',', "\\,")
                .replace('\n', "\\n")
        };
        let cn = |person: &Person| {
            person
                .name
                .as_deref()
                .map(|name| format!(";CN=\"{}\"", name.replace('"', "")))
                .unwrap_or_default()
        };
        let stamp = utc_stamp(now);
        let mut lines = vec![
            "BEGIN:VCALENDAR".to_owned(),
            "PRODID:-//Regards//Regards//EN".to_owned(),
            "VERSION:2.0".to_owned(),
            "METHOD:REPLY".to_owned(),
            "BEGIN:VEVENT".to_owned(),
            format!("UID:{}", self.uid),
            format!("SEQUENCE:{}", self.sequence),
            format!("DTSTAMP:{stamp}"),
        ];
        lines.extend(self.recurrence_id.clone());
        lines.extend(self.start_line.clone());
        if let Some(organizer) = &self.organizer {
            lines.push(format!(
                "ORGANIZER{}:mailto:{}",
                cn(organizer),
                organizer.address.as_deref().unwrap_or_default()
            ));
        }
        lines.push(format!(
            "ATTENDEE;PARTSTAT={}{}:mailto:{}",
            answer.partstat(),
            cn(me),
            me.address.as_deref().unwrap_or_default()
        ));
        lines.push(format!("SUMMARY:{}", escape(&self.summary)));
        lines.push("END:VEVENT".to_owned());
        lines.push("END:VCALENDAR".to_owned());
        lines.iter().map(|line| fold(line)).collect::<Vec<_>>().join("\r\n") + "\r\n"
    }
}

/// A content line folded at 75 octets (RFC 5545 §3.1), never inside a character.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for c in line.chars() {
        let size = c.len_utf8();
        if width + size > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += size;
    }
    out
}

/// `20261025T094500Z` for Unix milliseconds.
fn utc_stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVITE: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Example//Calendar//EN\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\n\
BEGIN:VTIMEZONE\r\nTZID:Europe/Zurich\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:plan-42@example.org\r\nSEQUENCE:2\r\nDTSTART;TZID=Europe/Zurich:20261030T100000\r\n\
DTEND;TZID=Europe/Zurich:20261030T110000\r\nSUMMARY:Garden plot\\, planning\r\nLOCATION:Shed\\nby the gate\r\n\
ORGANIZER;CN=Lena Okafor:mailto:lena@example.net\r\n\
ATTENDEE;CN=\"Sam Rivera\";ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:MAILTO:sam@example.org\r\n\
ATTENDEE;PARTSTAT=ACCEPTED;CN=Lena Okafor:mailto:lena@example.net\r\n\
DESCRIPTION:A long description that goes on and on and is folded over two lines bec\r\n ause it is long\r\n\
BEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:Not the event\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn the_calendar_part_of_a_message_is_found_before_an_ics_file() {
        let source = b"From: lena@example.net\r\nTo: sam@example.org\r\nSubject: Invitation\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=m\r\n\r\n\
--m\r\nContent-Type: multipart/alternative; boundary=a\r\n\r\n\
--a\r\nContent-Type: text/plain\r\n\r\nYou are invited.\r\n\
--a\r\nContent-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\r\n\
BEGIN:VCALENDAR\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:inline\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n\
--a--\r\n\
--m\r\nContent-Type: application/ics; name=invite.ics\r\nContent-Disposition: attachment; filename=invite.ics\r\n\r\n\
BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:file\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n\
--m--\r\n";
        let found = parse(&in_message(source).unwrap()).unwrap();
        assert_eq!((found.uid.as_str(), found.method.as_str()), ("inline", "REQUEST"));
        assert!(in_message(b"Subject: hi\r\n\r\nNo invitation here.").is_none());
    }

    #[test]
    fn an_invitation_says_what_when_where_and_who() {
        let invitation = parse(INVITE).unwrap();
        assert_eq!(invitation.method, "REQUEST");
        assert_eq!(
            (invitation.uid.as_str(), invitation.sequence),
            ("plan-42@example.org", 2)
        );
        assert_eq!(
            invitation.summary, "Garden plot, planning",
            "unescaped, not the alarm's"
        );
        assert_eq!(invitation.location.as_deref(), Some("Shed\nby the gate"));
        let start = invitation.start.clone().unwrap();
        assert_eq!(
            (start.value.as_str(), start.zone.as_deref(), start.utc, start.all_day),
            ("20261030T100000", Some("Europe/Zurich"), false, false)
        );
        assert_eq!(
            invitation.organizer.as_ref().and_then(|o| o.name.as_deref()),
            Some("Lena Okafor")
        );
        assert_eq!(invitation.attendees.len(), 2);
        assert_eq!(invitation.status_of("SAM@example.org"), Some("NEEDS-ACTION"));
        assert!(!invitation.repeats);
    }

    #[test]
    fn whole_days_utc_times_and_what_is_not_an_event() {
        let day = parse("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:a\nDTSTART;VALUE=DATE:20261224\nRRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR")
            .unwrap();
        let start = day.start.unwrap();
        assert!(start.all_day && !start.utc && day.repeats);
        let utc =
            parse("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:b\nDTSTART:20261030T090000Z\nEND:VEVENT\nEND:VCALENDAR").unwrap();
        assert!(utc.start.unwrap().utc);
        assert_eq!(
            parse("BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:c\nEND:VTODO\nEND:VCALENDAR"),
            None
        );
        assert_eq!(parse("not a calendar"), None);
    }

    #[test]
    fn a_reply_answers_for_the_person_and_folds_long_lines() {
        let invitation = parse(INVITE).unwrap();
        let me = Person {
            name: Some("Sam Rivera".into()),
            address: Some("sam@example.org".into()),
        };
        let reply = invitation.reply(&me, Answer::Accepted, 1_793_350_800_000);
        assert!(reply.starts_with("BEGIN:VCALENDAR\r\n"), "{reply}");
        for line in [
            "METHOD:REPLY",
            "UID:plan-42@example.org",
            "SEQUENCE:2",
            "DTSTAMP:20261030T090000Z",
            "DTSTART;TZID=Europe/Zurich:20261030T100000",
            "ORGANIZER;CN=\"Lena Okafor\":mailto:lena@example.net",
            "ATTENDEE;PARTSTAT=ACCEPTED;CN=\"Sam Rivera\":mailto:sam@example.org",
            "SUMMARY:Garden plot\\, planning",
        ] {
            assert!(reply.contains(&format!("{line}\r\n")), "{line} missing:\n{reply}");
        }
        assert!(reply.lines().all(|line| line.len() <= 76), "{reply}");
        let long = fold(&"é".repeat(60));
        assert!(long.split("\r\n").all(|line| line.len() <= 76), "{long}");
        assert_eq!(parse(&reply).unwrap().method, "REPLY", "it reads back");
    }
}
