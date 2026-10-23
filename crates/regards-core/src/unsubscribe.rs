//! Leaving a mailing list from one of its messages: the ways its
//! `List-Unsubscribe` header gives (RFC 2369), and whether the sender takes a
//! one-click request (`List-Unsubscribe-Post`, RFC 8058).
//!
//! Only web addresses on named hosts are kept: an `https://` link, never
//! `http://`, an IP address or this computer. A message could otherwise make
//! the app post to a device on the home network.

use mail_parser::MessageParser;

/// What a message offers to stop more like it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ways {
    /// RFC 8058: this `https://` address takes a POST of
    /// `List-Unsubscribe=One-Click`, and that is all it needs.
    pub one_click: Option<String>,
    /// An address to write to, with what the link asks the message to say.
    pub mailto: Option<Mailto>,
    /// A web page that unsubscribes once opened (and perhaps confirmed there).
    pub web: Option<String>,
}

impl Ways {
    pub fn any(&self) -> bool {
        self.one_click.is_some() || self.mailto.is_some() || self.web.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailto {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// The ways a message's headers (or whole source) give.
pub fn ways(raw: &[u8]) -> Ways {
    let Some(message) = MessageParser::default().parse_headers(raw) else {
        return Ways::default();
    };
    let Some(links) = message.header_raw("List-Unsubscribe") else {
        return Ways::default();
    };
    let one_click_asked = message
        .header_raw("List-Unsubscribe-Post")
        .is_some_and(|value| unfold(value).eq_ignore_ascii_case("List-Unsubscribe=One-Click"));
    let mut ways = Ways::default();
    for link in bracketed(&unfold(links)) {
        let lower = link.to_ascii_lowercase();
        if lower.starts_with("https://") && ways.web.is_none() && safe_host(&link) {
            ways.web = Some(link.clone());
        } else if let Some(rest) = lower.strip_prefix("mailto:").map(|_| &link[7..])
            && ways.mailto.is_none()
        {
            ways.mailto = mailto(rest);
        }
    }
    if one_click_asked {
        ways.one_click = ways.web.take();
    }
    ways
}

/// A header value on one line.
fn unfold(value: &str) -> String {
    value
        .split(['\r', '\n'])
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

/// The `<…>` entries of a list header, in order.
fn bracketed(value: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else { break };
        let inner: String = rest[start + 1..start + end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if !inner.is_empty() {
            found.push(inner);
        }
        rest = &rest[start + end + 1..];
    }
    found
}

/// An `https://` link whose host is a name with a dot: not an IP address,
/// not `localhost`, nothing that ends the request line.
fn safe_host(link: &str) -> bool {
    let after = &link["https://".len()..];
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    let ip = host.parse::<std::net::Ipv4Addr>().is_ok() || host.starts_with('[');
    !ip && host.contains('.')
        && !host.ends_with(".local")
        && !host.ends_with(".localhost")
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !link.chars().any(char::is_control)
}

/// What follows `mailto:`: one address, then `?subject=…&body=…`.
fn mailto(rest: &str) -> Option<Mailto> {
    let (address, fields) = rest.split_once('?').unwrap_or((rest, ""));
    let to = decode(address)?.trim().to_owned();
    let valid = to
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && !to
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, ',' | '<' | '>'));
    if !valid {
        return None;
    }
    let mut found = Mailto {
        to,
        subject: "unsubscribe".into(),
        body: String::new(),
    };
    for pair in fields.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value).unwrap_or_default();
        let value: String = value
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(1000)
            .collect();
        match name.to_ascii_lowercase().as_str() {
            "subject" if !value.trim().is_empty() => found.subject = value.trim().to_owned(),
            "body" => found.body = value,
            _ => {}
        }
    }
    Some(found)
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(headers: &str) -> Ways {
        ways(format!("{headers}\r\nSubject: News\r\n\r\nHello\r\n").as_bytes())
    }

    #[test]
    fn one_click_needs_the_post_header_and_a_web_link() {
        let ways = of(
            "List-Unsubscribe: <mailto:leave@news.example?subject=stop>,\r\n <https://news.example/u?id=7>\r\n\
                       List-Unsubscribe-Post: List-Unsubscribe=One-Click",
        );
        assert_eq!(ways.one_click.as_deref(), Some("https://news.example/u?id=7"));
        assert_eq!(ways.web, None);
        assert_eq!(
            ways.mailto,
            Some(Mailto {
                to: "leave@news.example".into(),
                subject: "stop".into(),
                body: String::new(),
            })
        );
        // Without it, the link is a page to open.
        let page = of("List-Unsubscribe: <https://news.example/u>");
        assert_eq!(
            (page.one_click, page.web.as_deref()),
            (None, Some("https://news.example/u"))
        );
    }

    #[test]
    fn mailto_links_say_what_they_ask_for() {
        let ways = of("List-Unsubscribe: <mailto:u%2Bkey@lists.example?subject=Unsubscribe%20me&body=please>");
        assert_eq!(
            ways.mailto,
            Some(Mailto {
                to: "u+key@lists.example".into(),
                subject: "Unsubscribe me".into(),
                body: "please".into(),
            })
        );
        assert_eq!(
            of("List-Unsubscribe: <mailto:leave@x.example>").mailto.unwrap().subject,
            "unsubscribe"
        );
        assert!(of("List-Unsubscribe: <mailto:not an address>").mailto.is_none());
    }

    #[test]
    fn only_https_links_to_named_hosts_are_kept() {
        for unsafe_link in [
            "http://news.example/u",
            "https://192.168.1.1/u",
            "https://[::1]/u",
            "https://localhost/u",
            "https://printer.local/u",
            "https://router/u",
            "javascript:alert(1)",
        ] {
            let ways = of(&format!(
                "List-Unsubscribe: <{unsafe_link}>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click"
            ));
            assert!(!ways.any(), "{unsafe_link} was kept: {ways:?}");
        }
        assert!(!of("From: a@b.example").any(), "no header, no way");
    }
}
