//! `account.discover` (plan §5.1): an address's mail servers, found before
//! the person types any, in the order of [`regards_core::autoconfig`]:
//! known providers, Thunderbird's database (shipped with Regards), MX records
//! (hosted domains), the settings file the domain publishes, the database by
//! MX host, SRV records, then the usual names on the domain; servers probed
//! with TLS so that a host is offered only if it answers with a valid
//! certificate.

use crate::{State, accounts};
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use regards_core::autoconfig::{self, ConfigFile, Endpoint, Security, Settings};
use regards_imap::{ImapServer, TlsOptions};
use regards_ipc::protocol::{
    AccountCheckFailure, CheckedServer, ConnectionSecurity, DiscoverParams, DiscoverResult, DiscoverSource, RpcError,
    ServerCheckParams, ServerCheckResult, ServerSettings, codes,
};
use regards_smtp::SmtpServer;
use serde_json::{Value, json};
use std::future::Future;
use std::time::Duration;
use tokio::task::JoinSet;

/// The whole search, at most.
const TOTAL: Duration = Duration::from_secs(12);
/// One DNS question.
const DNS_WAIT: Duration = Duration::from_secs(4);
/// A provider's settings file, from one of its two places.
const FILE_WAIT: Duration = Duration::from_secs(4);
/// One server's answer to a probe.
const PROBE_WAIT: Duration = Duration::from_secs(6);

pub(crate) async fn discover(state: &State, params: DiscoverParams) -> Result<Value, RpcError> {
    let domain = autoconfig::domain_of(&params.email)
        .ok_or_else(|| RpcError::new(codes::INVALID_PARAMS, "enter a valid email address"))?;
    let found = tokio::time::timeout(TOTAL, find(&state.tls, &domain))
        .await
        .unwrap_or_default();
    tracing::info!(found = ?found.source, "looked for an address's servers");
    Ok(json!(found))
}

async fn find(tls: &TlsOptions, domain: &str) -> DiscoverResult {
    if let Some(provider) = autoconfig::no_imap(domain) {
        return no_imap(provider, DiscoverSource::Known);
    }
    if let Some(settings) = autoconfig::known(domain) {
        return result(settings, DiscoverSource::Known);
    }
    if let Some(config) = autoconfig::from_database(domain)
        && let Some(found) = database_entry(tls, config).await
    {
        return found;
    }
    // The domain's own settings file, asked while DNS answers.
    let file = tokio::spawn(provider_file(tls.clone(), domain.to_owned()));
    let dns = dns(domain).await;
    if let Some(dns) = &dns {
        if let Some(provider) = autoconfig::no_imap_by_mx(&dns.mx) {
            file.abort();
            return no_imap(provider, DiscoverSource::Mx);
        }
        if let Some(settings) = autoconfig::by_mx(&dns.mx) {
            file.abort();
            return result(settings, DiscoverSource::Mx);
        }
    }
    if let Ok(Some(config)) = file.await
        && let Some(found) = checked(tls, config, DiscoverSource::File).await
    {
        return found;
    }
    if let Some(dns) = &dns
        && let Some(config) = autoconfig::from_database_by_mx(&dns.mx, domain)
        && let Some(found) = database_entry(tls, config).await
    {
        return found;
    }
    if let Some(dns) = &dns
        && let Some((imap, smtp)) = autoconfig::from_srv(&dns.imaps, &dns.submissions, &dns.submission)
        && imap_answers(tls, &imap).await
        && smtp_answers(tls, &smtp).await
    {
        return result(plain(imap, smtp), DiscoverSource::Dns);
    }
    let (imaps, smtps) = autoconfig::guesses(domain);
    let (imap, smtp) = answering(tls, imaps, smtps).await;
    found(imap, smtp, DiscoverSource::Guess)
}

/// A provider from Thunderbird's database. When it names the servers of a
/// provider Regards lists (outlook.it is Microsoft's), Regards' own entry:
/// its sign-in and what to say about the password. Otherwise the servers
/// that answer.
async fn database_entry(tls: &TlsOptions, config: ConfigFile) -> Option<DiscoverResult> {
    let listed = config
        .imap
        .first()
        .and_then(|imap| autoconfig::known_by_server(&imap.host));
    match listed {
        Some(settings) => Some(result(settings, DiscoverSource::Database)),
        None => checked(tls, config, DiscoverSource::Database).await,
    }
}

/// The servers a settings file names that answer, under its provider's
/// name; `None` when no IMAP server answered.
async fn checked(tls: &TlsOptions, config: ConfigFile, source: DiscoverSource) -> Option<DiscoverResult> {
    let (imap, smtp) = answering(tls, config.imap, config.smtp).await;
    imap.is_some().then(|| DiscoverResult {
        provider_name: config.name,
        ..found(imap, smtp, source)
    })
}

/// What DNS says of a domain's mail: its MX host names, and its SRV records
/// for IMAP over TLS and for submission (TLS, then STARTTLS).
struct Dns {
    mx: Vec<String>,
    imaps: Vec<(u16, String, u16)>,
    submissions: Vec<(u16, String, u16)>,
    submission: Vec<(u16, String, u16)>,
}

async fn dns(domain: &str) -> Option<Dns> {
    let resolver = TokioResolver::builder_tokio()
        .and_then(|builder| builder.build())
        .ok()?;
    let names = [
        format!("_imaps._tcp.{domain}."),
        format!("_submissions._tcp.{domain}."),
        format!("_submission._tcp.{domain}."),
    ];
    let (mx, imaps, submissions, submission) = tokio::join!(
        mx(&resolver, domain),
        srv(&resolver, &names[0]),
        srv(&resolver, &names[1]),
        srv(&resolver, &names[2]),
    );
    Some(Dns {
        mx,
        imaps,
        submissions,
        submission,
    })
}

/// The settings file the domain publishes, from the first of its two places
/// with one Regards can use; both asked at once.
async fn provider_file(tls: TlsOptions, domain: String) -> Option<ConfigFile> {
    let [first, second] = autoconfig::config_urls(&domain);
    let other = tokio::spawn({
        let (tls, domain) = (tls.clone(), domain.clone());
        async move { settings_file(&tls, &second, &domain).await }
    });
    if let Some(found) = settings_file(&tls, &first, &domain).await {
        other.abort();
        return Some(found);
    }
    other.await.ok().flatten()
}

async fn settings_file(tls: &TlsOptions, url: &str, domain: &str) -> Option<ConfigFile> {
    let answer = tokio::time::timeout(FILE_WAIT, regards_net::https::get(url, tls))
        .await
        .ok()?
        .ok()?;
    if answer.status != 200 {
        return None;
    }
    autoconfig::from_config(std::str::from_utf8(&answer.body).ok()?, domain)
}

/// The first of each list that answers, all probed at once.
async fn answering(
    tls: &TlsOptions,
    imaps: Vec<Endpoint>,
    smtps: Vec<Endpoint>,
) -> (Option<Endpoint>, Option<Endpoint>) {
    tokio::join!(
        first_answering(imaps, |endpoint| {
            let tls = tls.clone();
            async move { imap_answers(&tls, &endpoint).await }
        }),
        first_answering(smtps, |endpoint| {
            let tls = tls.clone();
            async move { smtp_answers(&tls, &endpoint).await }
        }),
    )
}

/// Servers that answered, from `source`: nothing known of how they sign in.
fn found(imap: Option<Endpoint>, smtp: Option<Endpoint>, source: DiscoverSource) -> DiscoverResult {
    DiscoverResult {
        source: (imap.is_some() || smtp.is_some()).then_some(source),
        imap: imap.map(settings_of),
        smtp: smtp.map(settings_of),
        ..DiscoverResult::default()
    }
}

/// Servers named by DNS: nothing known of how they sign in.
fn plain(imap: Endpoint, smtp: Endpoint) -> Settings {
    Settings {
        name: None,
        imap,
        smtp,
        oauth: None,
        password_help: None,
        oauth_only: false,
    }
}

fn result(settings: Settings, source: DiscoverSource) -> DiscoverResult {
    DiscoverResult {
        source: Some(source),
        provider_name: settings.name.map(str::to_owned),
        imap: Some(settings_of(settings.imap)),
        smtp: Some(settings_of(settings.smtp)),
        oauth: settings.oauth,
        oauth_only: settings.oauth_only,
        password_help: settings.password_help.map(str::to_owned),
        no_imap: None,
    }
}

/// A provider that lets no mail app in (its name, and why): no servers to offer.
fn no_imap((name, why): (&str, &str), source: DiscoverSource) -> DiscoverResult {
    DiscoverResult {
        source: Some(source),
        provider_name: Some(name.to_owned()),
        no_imap: Some(why.to_owned()),
        ..DiscoverResult::default()
    }
}

fn settings_of(endpoint: Endpoint) -> ServerSettings {
    ServerSettings {
        host: endpoint.host,
        port: endpoint.port,
        security: match endpoint.security {
            Security::Tls => ConnectionSecurity::Tls,
            Security::Starttls => ConnectionSecurity::Starttls,
        },
    }
}

async fn mx(resolver: &TokioResolver, domain: &str) -> Vec<String> {
    let Ok(Ok(found)) = tokio::time::timeout(DNS_WAIT, resolver.mx_lookup(format!("{domain}."))).await else {
        return Vec::new();
    };
    found
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::MX(mx) => Some(mx.exchange.to_ascii()),
            _ => None,
        })
        .collect()
}

async fn srv(resolver: &TokioResolver, name: &str) -> Vec<(u16, String, u16)> {
    let Ok(Ok(found)) = tokio::time::timeout(DNS_WAIT, resolver.srv_lookup(name)).await else {
        return Vec::new();
    };
    found
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::SRV(srv) => Some((srv.priority, srv.target.to_ascii(), srv.port)),
            _ => None,
        })
        .collect()
}

async fn imap_answers(tls: &TlsOptions, endpoint: &Endpoint) -> bool {
    imap_probe(tls, endpoint).await.is_ok()
}

async fn smtp_answers(tls: &TlsOptions, endpoint: &Endpoint) -> bool {
    smtp_probe(tls, endpoint).await.is_ok()
}

async fn imap_probe(tls: &TlsOptions, endpoint: &Endpoint) -> Result<(), AccountCheckFailure> {
    let server = ImapServer {
        host: endpoint.host.clone(),
        port: endpoint.port,
    };
    match tokio::time::timeout(PROBE_WAIT, regards_imap::probe(&server, tls)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => Err(accounts::imap_failure(&error)),
        Err(_) => Err(AccountCheckFailure::Timeout),
    }
}

async fn smtp_probe(tls: &TlsOptions, endpoint: &Endpoint) -> Result<(), AccountCheckFailure> {
    let server = SmtpServer {
        host: endpoint.host.clone(),
        port: endpoint.port,
        security: match endpoint.security {
            Security::Tls => regards_smtp::Security::Tls,
            Security::Starttls => regards_smtp::Security::StartTls,
        },
    };
    match tokio::time::timeout(PROBE_WAIT, regards_smtp::probe(&server, tls)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(accounts::smtp_failure(&error)),
        Err(_) => Err(AccountCheckFailure::Timeout),
    }
}

/// `account.check_server`: whether one server answers, with a certificate
/// valid for it, before any password is typed (the server form checks each
/// one as it is left). Nothing signs in. For a sending server that does not
/// answer, the other usual settings on the same host that do.
pub(crate) async fn check_server(state: &State, params: ServerCheckParams) -> Result<Value, RpcError> {
    let (imap, label) = match params.server {
        CheckedServer::Imap => (true, "IMAP"),
        CheckedServer::Smtp => (false, "SMTP"),
    };
    if imap && params.settings.security != ConnectionSecurity::Tls {
        return Err(RpcError::new(codes::INVALID_PARAMS, accounts::IMAP_TLS_ONLY));
    }
    let server = accounts::server(params.settings, label)?;
    let endpoint = Endpoint {
        host: server.host,
        port: server.port,
        security: match server.security {
            regards_store::accounts::Security::Tls => Security::Tls,
            regards_store::accounts::Security::Starttls => Security::Starttls,
        },
    };
    if imap {
        let reason = imap_probe(&state.tls, &endpoint).await.err();
        return Ok(json!(ServerCheckResult {
            reason,
            suggestion: None
        }));
    }
    // The same port with the other security (465 picked with STARTTLS), then
    // the two usual ones, all tried at once with what was typed.
    let mut others: Vec<Endpoint> = Vec::new();
    for (port, security) in [
        (endpoint.port, other(endpoint.security)),
        (587, Security::Starttls),
        (465, Security::Tls),
    ] {
        let tried = (port, security) == (endpoint.port, endpoint.security)
            || others
                .iter()
                .any(|known| (known.port, known.security) == (port, security));
        if !tried {
            others.push(Endpoint {
                host: endpoint.host.clone(),
                port,
                security,
            });
        }
    }
    let tls = state.tls.clone();
    let looking = tokio::spawn(first_answering(others, move |endpoint| {
        let tls = tls.clone();
        async move { smtp_answers(&tls, &endpoint).await }
    }));
    let reason = smtp_probe(&state.tls, &endpoint).await.err();
    let suggestion = if reason.is_some() {
        looking.await.ok().flatten().map(settings_of)
    } else {
        // What was typed answers: nothing to offer, no need to wait.
        looking.abort();
        None
    };
    Ok(json!(ServerCheckResult { reason, suggestion }))
}

fn other(security: Security) -> Security {
    match security {
        Security::Tls => Security::Starttls,
        Security::Starttls => Security::Tls,
    }
}

/// All probed at once; the first in `candidates`' order that answered,
/// as soon as every one before it has failed (the rest are dropped).
async fn first_answering<F, Fut>(candidates: Vec<Endpoint>, answers: F) -> Option<Endpoint>
where
    F: Fn(Endpoint) -> Fut,
    Fut: Future<Output = bool> + Send + 'static,
{
    let mut probes = JoinSet::new();
    for (index, endpoint) in candidates.iter().enumerate() {
        let probe = answers(endpoint.clone());
        probes.spawn(async move { (index, probe.await) });
    }
    // Per candidate: `None` while its probe runs, then whether it answered.
    let mut answered: Vec<Option<bool>> = vec![None; candidates.len()];
    while let Some(done) = probes.join_next().await {
        if let Ok((index, outcome)) = done {
            answered[index] = Some(outcome);
        }
        match answered.iter().position(|outcome| *outcome != Some(false)) {
            Some(best) if answered[best] == Some(true) => return Some(candidates[best].clone()),
            None => return None,
            Some(_) => {}
        }
    }
    // A probe that never finished (a panic) does not hold the others back.
    let best = answered.iter().position(|outcome| *outcome == Some(true))?;
    Some(candidates[best].clone())
}
