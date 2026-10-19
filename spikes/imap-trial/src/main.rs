//! Phase-0 spike for ADR-0009: async-imap vs imap-next against a real Dovecot,
//! plus a hostile-server robustness test. Not product code.
//!
//! Usage:
//!   imap-trial live            # both libraries against TRIAL_ADDR (TLS, CA in TRIAL_CA)
//!   imap-trial garbage [N]     # fixed malformed cases + N fuzz seeds per library (default 1000)

mod common;
mod garbage;
mod with_async_imap;
mod with_imap_next;

use std::{collections::BTreeMap, process::Command};

use anyhow::{Context, Result, bail};

const LIBS: [&str; 2] = ["async-imap", "imap-next"];

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("live") => live().await,
        Some("garbage") => garbage_parent(args.get(1).map(|n| n.parse()).transpose()?.unwrap_or(1000)),
        // Child-process entry point: one library, cases [start, end).
        Some("garbage-child") => garbage_child(&args[1], &args[2], args[3].parse()?, args[4].parse()?).await,
        _ => bail!("usage: imap-trial live | garbage [seeds]"),
    }
}

async fn live() -> Result<()> {
    let cfg = common::Config::from_env()?;
    for lib in LIBS {
        let mut report = common::Report::new(lib);
        let result = match lib {
            "async-imap" => with_async_imap::run(&cfg, &mut report).await,
            _ => with_imap_next::run(&cfg, &mut report).await,
        };
        if let Err(e) = result {
            report.add(
                "(scenario aborted)",
                std::time::Instant::now(),
                common::Outcome::Fail,
                format!("{e:#}"),
            );
        }
        report.print();
    }
    Ok(())
}

async fn garbage_child(lib: &str, kind: &str, start: u64, end: u64) -> Result<()> {
    garbage::install_panic_hook();
    for i in start..end {
        let case = if kind == "fixed" {
            garbage::fixed_cases().swap_remove(i as usize)
        } else {
            garbage::fuzz_case(i)
        };
        let name = case.name.clone();
        // Printed first, so the parent knows which case killed the process.
        println!("START\t{i}\t{name}");
        let (outcome, detail) = garbage::run_case(lib, case).await;
        println!(
            "RESULT\t{i}\t{name}\t{outcome}\t{}",
            detail.replace(['\t', '\n', '\r'], " ")
        );
    }
    Ok(())
}

struct CaseResult {
    name: String,
    outcome: String,
    detail: String,
}

/// Runs cases [0, total) in child processes, restarting after any process abort.
fn run_children(lib: &str, kind: &str, total: u64) -> Result<Vec<CaseResult>> {
    let exe = std::env::current_exe()?;
    let mut results = Vec::new();
    let mut start = 0;
    while start < total {
        let out = Command::new(&exe)
            .args(["garbage-child", lib, kind, &start.to_string(), &total.to_string()])
            .output()
            .context("running child")?;
        let mut pending: Option<(u64, String)> = None;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            match line.split('\t').collect::<Vec<_>>().as_slice() {
                ["START", i, name] => pending = Some((i.parse()?, name.to_string())),
                ["RESULT", i, name, outcome, detail] => {
                    pending = None;
                    start = i.parse::<u64>()? + 1;
                    results.push(CaseResult {
                        name: name.to_string(),
                        outcome: outcome.to_string(),
                        detail: detail.to_string(),
                    });
                }
                _ => {}
            }
        }
        match pending {
            Some((i, name)) => {
                results.push(CaseResult {
                    name,
                    outcome: "PROCESS ABORTED".into(),
                    detail: out.status.to_string(),
                });
                start = i + 1;
            }
            None if start < total => bail!("{lib} {kind} child stopped early: {}", out.status),
            None => {}
        }
    }
    Ok(results)
}

fn garbage_parent(seeds: u64) -> Result<()> {
    let fixed_total = garbage::fixed_cases().len() as u64;
    let mut fixed = Vec::new();
    let mut fuzz = Vec::new();
    for lib in LIBS {
        fixed.push(run_children(lib, "fixed", fixed_total)?);
        fuzz.push(run_children(lib, "fuzz", seeds)?);
    }

    println!("\n## Malformed server, fixed cases\n");
    println!("| Case | {} | {} |", LIBS[0], LIBS[1]);
    println!("| --- | --- | --- |");
    for i in 0..fixed_total as usize {
        let cell = |r: &CaseResult| {
            let outcome = if r.outcome == "error" {
                "error".to_string()
            } else {
                format!("**{}**", r.outcome)
            };
            format!("{outcome}<br><sub>{}</sub>", short(&r.detail))
        };
        println!(
            "| {} | {} | {} |",
            fixed[0][i].name,
            cell(&fixed[0][i]),
            cell(&fixed[1][i])
        );
    }
    println!("\n## Malformed server, {seeds} fuzz seeds per library\n");
    for (lib, results) in LIBS.iter().zip(&fuzz) {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for r in results {
            *counts.entry(r.outcome.as_str()).or_default() += 1;
        }
        println!("- **{lib}**: {counts:?}");
        let notable: Vec<_> = results
            .iter()
            .filter(|r| !["error", "accepted", "recovered"].contains(&r.outcome.as_str()))
            .collect();
        for r in notable.iter().take(10) {
            println!("  - {}: {} {}", r.name, r.outcome, short(&r.detail));
        }
        if notable.len() > 10 {
            println!("  - … {} more", notable.len() - 10);
        }
    }
    Ok(())
}

fn short(detail: &str) -> String {
    let d: String = detail.chars().take(90).collect();
    if detail.chars().count() > 90 {
        format!("{d}…")
    } else {
        d
    }
}
