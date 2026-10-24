//! Finding an address's mail servers (plan §5.1), so adding an account asks
//! for the address and a password, not for host names and ports.
//!
//! In order, the first that answers wins:
//!
//! 1. **Known providers** ([`known`]): the big ones, by the address's domain;
//!    and those that let no mail app in ([`no_imap`]), to say so.
//! 2. **DNS** (the daemon asks): RFC 6186/8314 SRV records the domain
//!    publishes ([`from_srv`]), or its MX records naming a provider that
//!    hosts other domains, such as Microsoft 365 or Google Workspace, or
//!    its own addresses in other countries ([`by_mx`], [`no_imap_by_mx`]).
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
    /// What people know the provider as ("Infomaniak", "Google
    /// Workspace"), for the form to say; `None` for servers found by DNS
    /// or by their usual names.
    pub name: Option<&'static str>,
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
    name: &'static str,
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
        name: "Gmail",
        domains: &["gmail.com", "googlemail.com"],
        imap: ("imap.gmail.com", 993),
        smtp: ("smtp.gmail.com", 587, Security::Starttls),
        oauth: Some(Provider::Google),
        password_help: Some(GOOGLE_HELP),
        oauth_only: false,
    },
    Known {
        name: "Outlook.com",
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
        name: "iCloud Mail",
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
        name: "Yahoo Mail",
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
        name: "AOL Mail",
        domains: &["aol.com", "aim.com", "aol.fr", "aol.de", "aol.it", "aol.co.uk"],
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
        name: "Fastmail",
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
        name: "Infomaniak",
        domains: &["ik.me", "etik.com", "ikmail.com"],
        imap: ("mail.infomaniak.com", 993),
        smtp: ("mail.infomaniak.com", 587, Security::Starttls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "GMX",
        domains: &["gmx.net", "gmx.de", "gmx.ch", "gmx.at"],
        imap: ("imap.gmx.net", 993),
        smtp: ("mail.gmx.net", 587, Security::Starttls),
        oauth: None,
        password_help: Some("GMX lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        name: "WEB.DE",
        domains: &["web.de"],
        imap: ("imap.web.de", 993),
        smtp: ("smtp.web.de", 587, Security::Starttls),
        oauth: None,
        password_help: Some("WEB.DE lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        name: "Bluewin",
        domains: &["bluewin.ch"],
        imap: ("imaps.bluewin.ch", 993),
        smtp: ("smtpauths.bluewin.ch", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "GMX",
        domains: &["gmx.com", "gmx.us", "gmx.co.uk", "gmx.fr", "gmx.es", "gmx.it"],
        imap: ("imap.gmx.com", 993),
        smtp: ("mail.gmx.com", 587, Security::Starttls),
        oauth: None,
        password_help: Some("GMX lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        name: "mail.com",
        domains: &["mail.com", "email.com", "usa.com", "post.com"],
        imap: ("imap.mail.com", 993),
        smtp: ("smtp.mail.com", 587, Security::Starttls),
        oauth: None,
        password_help: Some("mail.com lets mail apps in only once IMAP is turned on in its settings."),
        oauth_only: false,
    },
    Known {
        name: "T-Online",
        domains: &["t-online.de", "magenta.de"],
        imap: ("secureimap.t-online.de", 993),
        smtp: ("securesmtp.t-online.de", 465, Security::Tls),
        oauth: None,
        password_help: Some(
            "T-Online takes its own email password, not your Telekom login password: set one in the \
             Telekom E-Mail Center settings.",
        ),
        oauth_only: false,
    },
    Known {
        name: "Posteo",
        domains: &[
            "posteo.de",
            "posteo.net",
            "posteo.at",
            "posteo.ch",
            "posteo.eu",
            "posteo.org",
            "posteo.com",
        ],
        imap: ("posteo.de", 993),
        smtp: ("posteo.de", 587, Security::Starttls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "mailbox.org",
        domains: &["mailbox.org"],
        imap: ("imap.mailbox.org", 993),
        smtp: ("smtp.mailbox.org", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "Zoho Mail",
        domains: &["zoho.com", "zohomail.com"],
        imap: ("imap.zoho.com", 993),
        smtp: ("smtp.zoho.com", 465, Security::Tls),
        oauth: None,
        password_help: Some(ZOHO_HELP),
        oauth_only: false,
    },
    Known {
        name: "Zoho Mail",
        domains: &["zoho.eu", "zohomail.eu"],
        imap: ("imap.zoho.eu", 993),
        smtp: ("smtp.zoho.eu", 465, Security::Tls),
        oauth: None,
        password_help: Some(ZOHO_HELP),
        oauth_only: false,
    },
    Known {
        name: "Yandex Mail",
        domains: &["yandex.com", "yandex.ru", "ya.ru"],
        imap: ("imap.yandex.com", 993),
        smtp: ("smtp.yandex.com", 465, Security::Tls),
        oauth: None,
        password_help: Some(
            "Yandex Mail takes an app password: create one in your Yandex ID under Security, and allow \
             mail apps in Yandex Mail's settings.",
        ),
        oauth_only: false,
    },
    Known {
        name: "Orange",
        domains: &["orange.fr", "wanadoo.fr"],
        imap: ("imap.orange.fr", 993),
        smtp: ("smtp.orange.fr", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "Free",
        domains: &["free.fr"],
        imap: ("imap.free.fr", 993),
        smtp: ("smtp.free.fr", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "SFR",
        domains: &["sfr.fr", "neuf.fr"],
        imap: ("imap.sfr.fr", 993),
        smtp: ("smtp.sfr.fr", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
    Known {
        name: "La Poste",
        domains: &["laposte.net"],
        imap: ("imap.laposte.net", 993),
        smtp: ("smtp.laposte.net", 465, Security::Tls),
        oauth: None,
        password_help: None,
        oauth_only: false,
    },
];

const ZOHO_HELP: &str = "Zoho Mail lets mail apps in once IMAP access is turned on in its settings; with \
     two-factor sign-in, it takes an app-specific password.";

impl Known {
    fn settings(&self) -> Settings {
        Settings {
            name: Some(self.name),
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

/// Whose domains an MX rule finds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hosts {
    /// The provider's own, such as its addresses in other countries: they
    /// sign in as its listed domain does, under its name.
    Own,
    /// Other organisations', under this name (Microsoft 365, Google
    /// Workspace): how their people sign in is the organisation's choice.
    Others(&'static str),
}

/// Providers by the end of their MX host names, most specific first.
const BY_MX: &[(&str, &str, Hosts)] = &[
    // Microsoft 365: <tenant>.mail.protection.outlook.com.
    (
        ".mail.protection.outlook.com",
        "outlook.com",
        Hosts::Others("Microsoft 365"),
    ),
    // Microsoft's own addresses in every country (hotmail.it, live.nl, outlook.es):
    // <region>.olc.protection.outlook.com.
    (".olc.protection.outlook.com", "outlook.com", Hosts::Own),
    // Google Workspace: aspmx.l.google.com and its alternates, or smtp.google.com.
    (".google.com", "gmail.com", Hosts::Others("Google Workspace")),
    (".googlemail.com", "gmail.com", Hosts::Others("Google Workspace")),
    (".icloud.com", "icloud.com", Hosts::Others("iCloud Mail")),
    (".messagingengine.com", "fastmail.com", Hosts::Others("Fastmail")),
    // AOL and the addresses it took over (verizon.net), before the rest of Yahoo.
    (".mail.gm0.yahoodns.net", "aol.com", Hosts::Own),
    (".yahoodns.net", "yahoo.com", Hosts::Others("Yahoo Mail")),
    (".infomaniak.ch", "ik.me", Hosts::Others("Infomaniak")),
    (".posteo.de", "posteo.de", Hosts::Own),
    (".mailbox.org", "mailbox.org", Hosts::Others("mailbox.org")),
    (".zoho.com", "zoho.com", Hosts::Others("Zoho Mail")),
    (".zoho.eu", "zoho.eu", Hosts::Others("Zoho Mail")),
];

/// A domain hosted by a provider Regards knows, by its MX records. For
/// Microsoft 365 and Google Workspace, passwords are the organisation's
/// choice: OAuth is offered, not required.
pub fn by_mx(mx_hosts: &[String]) -> Option<Settings> {
    mx_hosts.iter().find_map(|host| {
        let host = format!(".{}", host.trim_end_matches('.').to_ascii_lowercase());
        let (_, like, hosts) = BY_MX.iter().find(|(suffix, _, _)| host.ends_with(suffix))?;
        let mut settings = known(like)?;
        if let Hosts::Others(name) = *hosts {
            settings.name = Some(name);
            if settings.oauth.is_some() {
                settings.oauth_only = false;
                settings.password_help = None;
            }
        }
        Some(settings)
    })
}

/// A provider that lets no mail app in over IMAP.
struct NoImap {
    name: &'static str,
    domains: &'static [&'static str],
    /// The end of its MX host names, for the domains it hosts.
    mx: &'static str,
    /// What to tell the person, instead of asking for servers that do not exist.
    why: &'static str,
}

const NO_IMAP: &[NoImap] = &[
    NoImap {
        name: "Proton Mail",
        domains: &["proton.me", "protonmail.com", "protonmail.ch", "pm.me"],
        mx: ".protonmail.ch",
        why: "Proton Mail lets mail apps in only through its Proton Mail Bridge app, which Regards does not \
              support yet.",
    },
    NoImap {
        name: "Tuta",
        domains: &[
            "tuta.com",
            "tutanota.com",
            "tutanota.de",
            "tutamail.com",
            "tuta.io",
            "keemail.me",
        ],
        mx: ".tutanota.de",
        why: "Tuta Mail does not let other mail apps in: it works only in Tuta's own apps.",
    },
    NoImap {
        name: "HEY",
        domains: &["hey.com"],
        mx: ".app.hey.com",
        why: "HEY does not let other mail apps in: it works only in HEY's own apps.",
    },
];

/// A provider that cannot be added, by the address's own domain: its name,
/// and why.
pub fn no_imap(domain: &str) -> Option<(&'static str, &'static str)> {
    NO_IMAP
        .iter()
        .find(|provider| provider.domains.contains(&domain))
        .map(|provider| (provider.name, provider.why))
}

/// A hosted domain that cannot be added, by its MX records: the provider's
/// name, and why.
pub fn no_imap_by_mx(mx_hosts: &[String]) -> Option<(&'static str, &'static str)> {
    mx_hosts.iter().find_map(|host| {
        let host = format!(".{}", host.trim_end_matches('.').to_ascii_lowercase());
        NO_IMAP
            .iter()
            .find(|provider| host.ends_with(provider.mx))
            .map(|provider| (provider.name, provider.why))
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
    fn a_providers_own_addresses_abroad_sign_in_as_at_home() {
        // hotmail.it, live.nl, outlook.es: Microsoft's own, so sign-in only.
        let regional = by_mx(&["eur.olc.protection.outlook.com".into()]).unwrap();
        assert!(regional.oauth == Some(Provider::Microsoft) && regional.oauth_only);
        assert!(regional.password_help.is_some());
        // aol.fr, verizon.net: AOL's servers, not the rest of Yahoo's.
        let aol = by_mx(&["mx-aol.mail.gm0.yahoodns.net.".into()]).unwrap();
        assert_eq!(aol.imap.host, "imap.aol.com");
        let yahoo = by_mx(&["mta5.am0.yahoodns.net".into()]).unwrap();
        assert_eq!(yahoo.imap.host, "imap.mail.yahoo.com");
        assert_eq!(by_mx(&["mx02.posteo.de".into()]).unwrap().imap.host, "posteo.de");
    }

    #[test]
    fn more_providers_by_their_domains() {
        let cases = [
            ("posteo.ch", "posteo.de"),
            ("mailbox.org", "imap.mailbox.org"),
            ("zohomail.eu", "imap.zoho.eu"),
            ("t-online.de", "secureimap.t-online.de"),
            ("gmx.com", "imap.gmx.com"),
            ("wanadoo.fr", "imap.orange.fr"),
            ("aol.fr", "imap.aol.com"),
        ];
        for (domain, imap) in cases {
            assert_eq!(known(domain).unwrap().imap.host, imap, "{domain}");
        }
        assert!(known("t-online.de").unwrap().password_help.is_some());
        let mut seen = std::collections::HashSet::new();
        for listed in KNOWN.iter().flat_map(|known| known.domains) {
            assert!(seen.insert(listed), "{listed} is listed twice");
        }
    }

    #[test]
    fn providers_that_let_no_mail_app_in_say_why() {
        let (name, why) = no_imap("proton.me").unwrap();
        assert!(name == "Proton Mail" && why.contains("Bridge"));
        assert!(no_imap("tutanota.com").is_some());
        assert_eq!(no_imap("gmail.com"), None);
        // A domain of their own, hosted there.
        assert!(no_imap_by_mx(&["mail.protonmail.ch.".into(), "mailsec.protonmail.ch.".into()]).is_some());
        assert_eq!(no_imap_by_mx(&["mail.tutanota.de".into()]).unwrap().0, "Tuta");
        assert_eq!(no_imap_by_mx(&["mx.example.org".into()]), None);
        // None of them is in the list of providers to add.
        for provider in NO_IMAP {
            assert!(provider.domains.iter().all(|domain| known(domain).is_none()));
        }
    }

    #[test]
    fn providers_by_the_name_people_know() {
        assert_eq!(known("ik.me").unwrap().name, Some("Infomaniak"));
        let hosted = |mx: &str| by_mx(&[mx.into()]).unwrap().name;
        assert_eq!(hosted("aspmx.l.google.com"), Some("Google Workspace"));
        assert_eq!(hosted("contoso-com.mail.protection.outlook.com"), Some("Microsoft 365"));
        assert_eq!(hosted("eur.olc.protection.outlook.com"), Some("Outlook.com"));
        assert_eq!(hosted("mta-gw.infomaniak.ch"), Some("Infomaniak"));
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
