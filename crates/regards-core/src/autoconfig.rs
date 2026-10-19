//! Finding an address's mail servers (plan §5.1), so adding an account asks
//! for the address and a password, not for host names and ports.
//!
//! In order, the first that answers wins:
//!
//! 1. **Known providers** ([`known`]): the big ones, by the address's domain.
//! 2. **DNS** (the daemon asks): RFC 6186/8314 SRV records the domain
//!    publishes ([`from_srv`]), or its MX records naming a provider that
//!    hosts other domains, such as Microsoft 365 or Google Workspace
//!    ([`by_mx`]).
//! 3. **The usual names** on the domain ([`guesses`]): `imap.`, `mail.`,
//!    `smtp.` with the standard ports, each checked before it is offered.
//!
//! Nothing here is sent anywhere: DNS goes to the system's resolvers, and
//! the only servers contacted are the candidates themselves. What is found
//! is only a proposal: adding the account still signs in to check it.

use crate::oauth::Provider;

/// How a connection is secured; IMAP is always TLS from the first byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Tls,
    Starttls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

/// An address's servers, and how it signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub imap: Endpoint,
    pub smtp: Endpoint,
    /// The provider's OAuth sign-in, if it has one Regards knows.
    pub oauth: Option<Provider>,
    /// It does not take the account's own password: only OAuth, or an app
    /// password made for Regards. What to tell the person, if anything.
    pub password_help: Option<&'static str>,
    /// Only OAuth works (Microsoft's consumer accounts since 2024).
    pub oauth_only: bool,
}

struct Known {
    domains: &'static [&'static str],
    imap: (&'static str, u16),
    smtp: (&'static str, u16, Security),
    oauth: Option<Provider>,
    password_help: Option<&'static str>,
    oauth_only: bool,
}

const GOOGLE_HELP: &str = "Gmail takes an app password, not your Google password: turn on 2-Step Verification, \
     then create one at myaccount.google.com/apppasswords.";

const KNOWN: &[Known] = &[
    Known {
        domains: &["gmail.com", "googlemail.com"],
        imap: ("imap.gmail.com", 993),
        smtp: ("smtp.gmail.com", 587, Security::Starttls),
        oauth: Some(Provider::Google),
        password_help: Some(GOOGLE_HELP),
        oauth_only: false,
    },
    Known {
        domains: &[
            "outlook.com",
            "hotmail.com",
            "live.com",
            "msn.com",
            "outlook.fr",
            "hotmail.fr",
            "live.fr",
            "hotmail.co.uk",
            "outlook.de",
            "hotmail.de",
        ],
        imap: ("outlook.office365.com", 993),
        smtp: ("smtp.office365.com", 587, Security::Starttls),
        oauth: Some(Provider::Microsoft),
        password_help: Some("Microsoft accounts sign in with Microsoft, not with a password."),
        oauth_only: true,
    },
    Known {
        domains: &["icloud.com", "me.com", "mac.com"],
        imap: ("imap.mail.me.com", 993),
        smtp: ("smtp.mail.me.com", 587, Security::Starttls),
        oauth: None,
        password_help: Some(
            "iCloud Mail takes an app-specific password, not your Apple Account password: create one \
             at account.apple.com, under Sign-In and Security.",
        ),
        oauth_only: false,
    },
    Known {
        domains: &[
            "yahoo.com",
            "yahoo.fr",
            "yahoo.co.uk",
            "yahoo.de",
            "ymail.com",
            "rocketmail.com",
        ],
        imap: ("imap.mail.yahoo.com", 993),
        smtp: ("smtp.mail.yahoo.com", 465, Security::Tls),
        oauth: None,
        password_help: Some(
            "Yahoo Mail takes an app password, not your Yahoo password: create one under Account \
             security in your Yahoo account.",
        ),
        oauth_only: false,
    },
    Known {
        domains: &["aol.com"],
        imap: ("imap.aol.com", 993),
        smtp: ("smtp.aol.com", 465, Security::Tls),
        oauth: None,
        password_help: Some(
            "AOL Mail takes an app password, not your AOL password: create one under Account \
             security in your AOL account.",
        ),
        oauth_only: false,
    },
    Known {
        domains: &["fastmail.com", "fastmail.fm"],
        imap: ("imap.fastmail.com", 993),
        smtp: ("smtp.fastmail.com", 465, Security::Tls),
        oauth: None,
        password_help: Some(
            "Fastmail takes an app password: create one in Fastmail under Settings › Privacy & Security.",
        ),
        oauth_only: false,
    },
    Known {
        domains: &["ik.me", "etik.com", "ikmail.com"],
        imap: ("mail.infomaniak.com", 993),
        smtp: ("mail.infomaniak.com", 587, Security::Starttls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        domains: &["gmx.net", "gmx.de", "gmx.ch", "gmx.at"],
        imap: ("imap.gmx.net", 993),
        smtp: ("mail.gmx.net", 587, Security::Starttls),
        oauth: None,
        password_help: Some("GMX lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        domains: &["web.de"],
        imap: ("imap.web.de", 993),
        smtp: ("smtp.web.de", 587, Security::Starttls),
        oauth: None,
        password_help: Some("WEB.DE lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        domains: &["bluewin.ch"],
        imap: ("imaps.bluewin.ch", 993),
        smtp: ("smtpauths.bluewin.ch", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
];

impl Known {
    fn settings(&self) -> Settings {
        Settings {
            imap: Endpoint {
                host: self.imap.0.to_owned(),
                port: self.imap.1,
                security: Security::Tls,
            },
            smtp: Endpoint {
                host: self.smtp.0.to_owned(),
                port: self.smtp.1,
                security: self.smtp.2,
            },
            oauth: self.oauth,
            password_help: self.password_help,
            oauth_only: self.oauth_only,
        }
    }
}

/// The address's domain, lower case.
pub fn domain_of(address: &str) -> Option<String> {
    let domain = address
        .trim()
        .rsplit_once('@')?
        .1
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (domain.contains('.') && !domain.chars().any(|c| c.is_whitespace() || c == '/')).then_some(domain)
}

/// A provider Regards knows, by the address's own domain.
pub fn known(domain: &str) -> Option<Settings> {
    KNOWN
        .iter()
        .find(|known| known.domains.contains(&domain))
        .map(Known::settings)
}

/// Providers that host other domains, by the end of their MX host names.
const BY_MX: &[(&str, &str)] = &[
    // Microsoft 365: <tenant>.mail.protection.outlook.com.
    (".mail.protection.outlook.com", "outlook.com"),
    // Google Workspace: aspmx.l.google.com and its alternates, or smtp.google.com.
    (".google.com", "gmail.com"),
    (".googlemail.com", "gmail.com"),
    (".icloud.com", "icloud.com"),
    (".messagingengine.com", "fastmail.com"),
    (".yahoodns.net", "yahoo.com"),
    (".infomaniak.ch", "ik.me"),
];

/// A domain hosted by a provider Regards knows, by its MX records. For
/// Microsoft 365 and Google Workspace, passwords are the organisation's
/// choice: OAuth is offered, not required.
pub fn by_mx(mx_hosts: &[String]) -> Option<Settings> {
    mx_hosts.iter().find_map(|host| {
        let host = format!(".{}", host.trim_end_matches('.').to_ascii_lowercase());
        let (_, like) = BY_MX.iter().find(|(suffix, _)| host.ends_with(suffix))?;
        let mut settings = known(like)?;
        if settings.oauth.is_some() {
            settings.oauth_only = false;
            settings.password_help = None;
        }
        Some(settings)
    })
}

/// What a domain's SRV records say (RFC 6186, RFC 8314): `_imaps._tcp` for
/// IMAP over TLS; `_submissions._tcp` (TLS) before `_submission._tcp`
/// (STARTTLS). Records are `(priority, target, port)`; a target of "." says
/// the service is not offered. IMAP without TLS (`_imap._tcp`) is not used.
pub fn from_srv(
    imaps: &[(u16, String, u16)],
    submissions: &[(u16, String, u16)],
    submission: &[(u16, String, u16)],
) -> Option<(Endpoint, Endpoint)> {
    let best = |records: &[(u16, String, u16)], security| {
        records
            .iter()
            .filter(|(_, target, port)| *port != 0 && !target.trim_end_matches('.').is_empty())
            .min_by_key(|(priority, _, _)| *priority)
            .map(|(_, target, port)| Endpoint {
                host: target.trim_end_matches('.').to_ascii_lowercase(),
                port: *port,
                security,
            })
    };
    let imap = best(imaps, Security::Tls)?;
    let smtp = best(submissions, Security::Tls).or_else(|| best(submission, Security::Starttls))?;
    Some((imap, smtp))
}

/// The usual names, best first, to try in turn: IMAP on 993; submission
/// with STARTTLS on 587, then TLS on 465.
pub fn guesses(domain: &str) -> (Vec<Endpoint>, Vec<Endpoint>) {
    let at = |name: &str, port, security| Endpoint {
        host: format!("{name}.{domain}"),
        port,
        security,
    };
    (
        vec![at("imap", 993, Security::Tls), at("mail", 993, Security::Tls)],
        vec![
            at("smtp", 587, Security::Starttls),
            at("mail", 587, Security::Starttls),
            at("smtp", 465, Security::Tls),
            at("mail", 465, Security::Tls),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_providers_by_their_own_domains() {
        assert_eq!(domain_of(" Ada@GMail.com "), Some("gmail.com".into()));
        assert_eq!(domain_of("ada@localhost"), None);
        let gmail = known("gmail.com").unwrap();
        assert_eq!(
            (gmail.imap.host.as_str(), gmail.imap.port, gmail.smtp.port),
            ("imap.gmail.com", 993, 587)
        );
        assert_eq!(gmail.oauth, Some(Provider::Google));
        let outlook = known("hotmail.fr").unwrap();
        assert!(outlook.oauth_only && outlook.oauth == Some(Provider::Microsoft));
        // The servers OAuth accounts get are the same.
        let ((imap, _), (smtp, _)) = Provider::Microsoft.servers();
        assert_eq!((outlook.imap.host.as_str(), outlook.smtp.host.as_str()), (imap, smtp));
        assert!(known("icloud.com").unwrap().password_help.is_some());
        assert_eq!(known("example.org"), None);
    }

    #[test]
    fn hosted_domains_by_their_mx() {
        let tenant = by_mx(&["contoso-com.mail.protection.outlook.com.".into()]).unwrap();
        assert_eq!(tenant.imap.host, "outlook.office365.com");
        assert!(
            tenant.oauth == Some(Provider::Microsoft) && !tenant.oauth_only,
            "a work account may still use a password"
        );
        let workspace = by_mx(&["mx.example.org".into(), "ALT1.ASPMX.L.GOOGLE.COM".into()]).unwrap();
        assert_eq!(workspace.smtp.host, "smtp.gmail.com");
        assert_eq!(workspace.password_help, None);
        let infomaniak = by_mx(&["mta-gw.infomaniak.ch".into()]).unwrap();
        assert_eq!(infomaniak.imap.host, "mail.infomaniak.com");
        assert_eq!(by_mx(&["mx.example.org".into()]), None);
        assert_eq!(by_mx(&["notgoogle.com".into()]), None, "a whole label, not any ending");
    }

    #[test]
    fn srv_records_name_the_servers() {
        let record = |priority, target: &str, port| (priority, target.to_owned(), port);
        let (imap, smtp) = from_srv(
            &[
                record(10, "imap2.example.org.", 993),
                record(0, "imap.example.org.", 993),
            ],
            &[],
            &[record(0, "smtp.example.org.", 587)],
        )
        .unwrap();
        assert_eq!((imap.host.as_str(), imap.port), ("imap.example.org", 993));
        assert_eq!(
            (smtp.host.as_str(), smtp.port, smtp.security),
            ("smtp.example.org", 587, Security::Starttls)
        );
        let (_, tls) = from_srv(
            &[record(0, "imap.example.org.", 993)],
            &[record(0, "smtp.example.org.", 465)],
            &[record(0, "smtp.example.org.", 587)],
        )
        .unwrap();
        assert_eq!((tls.port, tls.security), (465, Security::Tls), "implicit TLS first");
        assert_eq!(
            from_srv(&[record(0, ".", 0)], &[], &[record(0, "smtp.example.org.", 587)]),
            None,
            "\".\": not offered"
        );
    }

    #[test]
    fn the_usual_names_in_order() {
        let (imap, smtp) = guesses("example.org");
        assert_eq!(imap[0].host, "imap.example.org");
        assert_eq!((smtp[0].host.as_str(), smtp[0].port), ("smtp.example.org", 587));
        assert_eq!(smtp.last().unwrap().security, Security::Tls);
    }
}
