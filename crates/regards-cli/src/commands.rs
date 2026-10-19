//! The CLI's mail commands, made from [`tools::TOOLS`]: `regards search
//! garden`, `regards read 42`, `regards archive 42 43`. Each runs one tool;
//! with `--json` it prints the tool's result as is (the same JSON the MCP
//! server gives), otherwise a short text for people.

use crate::tools::{self, Kind, Outcome, Tool};
use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use regards_ipc::protocol::codes;
use serde_json::{Map, Value, json};
use std::process::ExitCode;

/// Exit codes of a tool that did not do what was asked.
const EXIT_NOT_ALLOWED: u8 = 5;
const EXIT_NOT_FOUND: u8 = 6;
const EXIT_INVALID: u8 = 2;

/// A subcommand for each tool.
pub(crate) fn all() -> Vec<Command> {
    tools::TOOLS.iter().map(command).collect()
}

fn command(tool: &Tool) -> Command {
    let mut command = Command::new(tool.name.replace('_', "-")).about(first_sentence(tool.description));
    command = command.long_about(format!(
        "{}

Needs the account's agent access: {}. {}",
        tool.description,
        tool.access.label(),
        if tool.read_only {
            "Changes nothing."
        } else {
            "Changes mail; shown in Regards › Settings › Agents."
        },
    ));
    for param in tool.params {
        let flag = param.name.replace('_', "-");
        let mut arg = Arg::new(param.name)
            .help(param.help)
            .value_name(param.name.to_uppercase());
        arg = if param.positional {
            arg.required(param.required)
        } else {
            arg.long(flag).required(param.required)
        };
        arg = match param.kind {
            Kind::Text => arg.value_parser(value_parser!(String)),
            Kind::Words => arg.num_args(1..).value_parser(value_parser!(String)),
            Kind::Integer => arg.value_parser(value_parser!(i64)),
            Kind::Integers => arg.num_args(1..).value_parser(value_parser!(i64)),
            Kind::Boolean => arg.value_parser(value_parser!(bool)),
            Kind::Flag => arg.action(ArgAction::SetTrue),
            Kind::People => arg.num_args(1..).value_parser(value_parser!(String)),
            Kind::Time => arg.value_parser(value_parser!(String)),
            Kind::OneOf(values) => arg.value_parser(values.to_vec()),
        };
        command = command.arg(arg);
    }
    command
}

fn first_sentence(text: &str) -> String {
    text.split_once(". ")
        .map_or(text, |(first, _)| first)
        .trim_end_matches('.')
        .to_owned()
}

/// The tool a subcommand runs, and its arguments as the tool takes them.
pub(crate) fn parse(name: &str, matches: &ArgMatches) -> Option<(&'static Tool, Map<String, Value>)> {
    let tool = tools::find(&name.replace('-', "_"))?;
    let mut args = Map::new();
    for param in tool.params {
        let value = match param.kind {
            Kind::Text | Kind::Time | Kind::OneOf(_) => matches.get_one::<String>(param.name).map(|v| json!(v)),
            Kind::Words => matches
                .get_many::<String>(param.name)
                .map(|words| json!(words.cloned().collect::<Vec<_>>().join(" "))),
            Kind::Integer => matches.get_one::<i64>(param.name).map(|v| json!(v)),
            Kind::Integers => matches
                .get_many::<i64>(param.name)
                .map(|ids| json!(ids.copied().collect::<Vec<_>>())),
            Kind::Boolean => matches.get_one::<bool>(param.name).map(|v| json!(v)),
            Kind::Flag => matches.get_flag(param.name).then_some(json!(true)),
            Kind::People => matches
                .get_many::<String>(param.name)
                .map(|people| json!(people.cloned().collect::<Vec<_>>())),
        };
        if let Some(value) = value {
            args.insert(param.name.to_owned(), value);
        }
    }
    Some((tool, args))
}

/// Print what a tool gave; the exit code to end with.
pub(crate) fn print(tool: &Tool, outcome: &Outcome, as_json: bool) -> ExitCode {
    match outcome {
        Outcome::Done(result) => {
            if as_json {
                println!("{result}");
            } else {
                println!("{}", human(tool, result));
            }
            ExitCode::SUCCESS
        }
        Outcome::Held(held) => {
            if as_json {
                println!("{}", json!({ "held": held }));
            } else {
                let description = held["description"].as_str().unwrap_or("this");
                let id = &held["hold_id"];
                println!(
                    "Waiting for your approval in Regards: {description} (hold {id}). \
                     See what became of it with `regards hold {id}`."
                );
            }
            ExitCode::SUCCESS
        }
        Outcome::Failed { code, message, data } => {
            if as_json {
                println!(
                    "{}",
                    json!({ "error": { "code": code, "message": message, "data": data } })
                );
            } else {
                eprintln!("error: {message}");
            }
            ExitCode::from(match *code {
                codes::FORBIDDEN => EXIT_NOT_ALLOWED,
                codes::NOT_FOUND => EXIT_NOT_FOUND,
                codes::INVALID_PARAMS => EXIT_INVALID,
                _ => 1,
            })
        }
    }
}

/// A result for people: lists as lines, a message as a letter.
fn human(tool: &Tool, result: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let person = |value: &Value| {
        let (name, address) = (text(&value["name"]), text(&value["address"]));
        if name.is_empty() {
            address
        } else {
            format!("{name} <{address}>")
        }
    };
    let people = |value: &Value| {
        value
            .as_array()
            .map(|list| list.iter().map(person).collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    };
    let line = |message: &Value| {
        let mut marks = String::new();
        if message["seen"] == json!(false) {
            marks.push('•');
        }
        if message["flagged"] == json!(true) {
            marks.push('⚑');
        }
        let date = text(&message["received_at"]);
        format!(
            "{:>7}  {:<16}  {:<24}  {}{}",
            message["id"],
            date.get(..16).unwrap_or(&date).replace('T', " "),
            person(&message["from"]).chars().take(24).collect::<String>(),
            text(&message["subject"]),
            if marks.is_empty() {
                String::new()
            } else {
                format!("  {marks}")
            },
        )
    };
    match tool.name {
        "people" => list(&result["people"], |person| {
            let address = text(&person["address"]);
            match person["name"].as_str().filter(|name| !name.is_empty()) {
                Some(name) => format!("{name} <{address}>"),
                None => address,
            }
        }),
        "accounts" => list(&result["accounts"], |account| {
            format!(
                "{}  {}  (agents: {})",
                text(&account["id"]),
                text(&account["email"]),
                text(&account["agent_access"])
            )
        }),
        "folders" => list(&result["mailboxes"], |folder| {
            let role = text(&folder["role"]);
            format!(
                "{:>5}  {}{}  {} unread",
                folder["id"],
                text(&folder["name"]),
                if role.is_empty() {
                    String::new()
                } else {
                    format!(" ({role})")
                },
                folder["unread"]
            )
        }),
        "messages" | "search" | "search_server" | "view" => {
            let mut out = list(&result["messages"], line);
            if result["next"].is_object() {
                out.push_str(&format!(
                    "\nMore: --before-received-at {} --before-id {}",
                    result["next"]["received_at"], result["next"]["id"]
                ));
            }
            if let Some(n) = result["not_here"].as_u64().filter(|n| *n > 0) {
                out.push_str(&format!("\n{n} more on the server, not on this device yet."));
            }
            if let Some(suggestion) = result["suggestion"].as_str() {
                out.push_str(&format!("\nDid you mean: {suggestion}"));
            }
            out
        }
        "read" => {
            let message = &result["message"];
            let mut out = format!(
                "From: {}\nTo: {}\n{}Date: {}\nSubject: {}\n\n{}",
                person(&message["from"]),
                people(&result["to"]),
                if result["cc"].as_array().is_some_and(|cc| !cc.is_empty()) {
                    format!("Cc: {}\n", people(&result["cc"]))
                } else {
                    String::new()
                },
                text(&message["received_at"]),
                text(&message["subject"]),
                text(&result["text"]).trim_end(),
            );
            if let Some(files) = result["attachments"].as_array().filter(|files| !files.is_empty()) {
                let names: Vec<String> = files.iter().map(|file| text(&file["name"])).collect();
                out.push_str(&format!("\n\nFiles: {}", names.join(", ")));
            }
            out
        }
        "thread" => list(&result["entries"], |entry| line(&entry["message"])),
        "drafts" => list(&result["drafts"], |draft| {
            format!(
                "{:>5}  {}  to {}",
                draft["id"],
                text(&draft["subject"]),
                people(&draft["to"])
            )
        }),
        "draft" | "write" | "reply" | "forward" | "edit" => format!(
            "Draft {}: “{}” to {}\n\n{}",
            result["id"],
            text(&result["subject"]),
            people(&result["to"]),
            text(&result["body"]).trim_end()
        ),
        "hold" => {
            let mut out = format!("{} — {}", text(&result["description"]), text(&result["status"]));
            if let Some(error) = result["error"].as_str() {
                out.push_str(&format!(": {error}"));
            }
            out
        }
        _ => match result.get("undo").and_then(|undo| undo.get("description")) {
            Some(done) => format!("{} (undoable in Regards).", text(done)),
            None => serde_json::to_string_pretty(result).unwrap_or_default(),
        },
    }
}

fn list(items: &Value, line: impl Fn(&Value) -> String) -> String {
    match items.as_array() {
        Some(items) if !items.is_empty() => items.iter().map(line).collect::<Vec<_>>().join("\n"),
        _ => "Nothing.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(line: &[&str]) -> (String, ArgMatches) {
        let app = Command::new("regards").subcommands(all());
        let matches = app.try_get_matches_from(line).unwrap();
        let (name, sub) = matches.subcommand().unwrap();
        (name.to_owned(), sub.clone())
    }

    #[test]
    fn command_lines_become_tool_arguments() {
        let (name, sub) = matches(&["regards", "search", "garden", "from:lena", "--limit", "5"]);
        let (tool, args) = parse(&name, &sub).unwrap();
        assert_eq!(tool.name, "search");
        assert_eq!(args["query"], "garden from:lena");
        assert_eq!(args["limit"], 5);

        let (name, sub) = matches(&["regards", "archive", "4", "5"]);
        assert_eq!(parse(&name, &sub).unwrap().1["message_ids"], json!([4, 5]));

        let (name, sub) = matches(&["regards", "mark", "4", "--read", "false"]);
        assert_eq!(parse(&name, &sub).unwrap().1["read"], false);

        let (name, sub) = matches(&[
            "regards",
            "write",
            "--account-id",
            "a1",
            "--to",
            "Bob <bob@example.test>",
            "ada@example.test",
            "--subject",
            "Plans",
        ]);
        let (tool, args) = parse(&name, &sub).unwrap();
        assert_eq!(tool.name, "write");
        assert_eq!(args["to"].as_array().unwrap().len(), 2);

        let (name, sub) = matches(&["regards", "set-aside", "9", "--aside", "true"]);
        assert_eq!(parse(&name, &sub).unwrap().0.name, "set_aside");
        let refused = Command::new("regards").subcommands(all()).try_get_matches_from([
            "regards",
            "view",
            "--account-id",
            "a1",
            "--view",
            "starred",
        ]);
        assert!(refused.is_err(), "only the views there are");
    }

    #[test]
    fn people_read_lists_and_letters() {
        let search = tools::find("search").unwrap();
        let listed = human(
            search,
            &json!({ "messages": [{
                "id": 42, "received_at": "2026-10-18T09:00:00.000Z", "seen": false, "flagged": false,
                "from": { "name": "Lena", "address": "lena@example.net" }, "subject": "Garden plot",
            }], "next": { "received_at": 1, "id": 42 } }),
        );
        assert!(
            listed.contains("42  2026-10-18 09:00  Lena <lena@example.net>"),
            "{listed}"
        );
        assert!(listed.contains("Garden plot  •") && listed.contains("--before-id 42"));
        let read = tools::find("read").unwrap();
        let letter = human(
            read,
            &json!({
                "message": { "from": { "name": null, "address": "bob@example.test" }, "subject": "Hi",
                             "received_at": "2026-10-18T09:00:00.000Z" },
                "to": [{ "name": "Ada", "address": "ada@example.test" }], "cc": [],
                "text": "Hello\n", "attachments": [{ "name": "plan.pdf" }],
            }),
        );
        assert!(
            letter.starts_with("From: bob@example.test\nTo: Ada <ada@example.test>\nDate:"),
            "{letter}"
        );
        assert!(letter.ends_with("Hello\n\nFiles: plan.pdf"));
        let archive = tools::find("archive").unwrap();
        assert_eq!(
            human(
                archive,
                &json!({ "moved": 2, "undo": { "id": 1, "description": "Archived 2 messages" } })
            ),
            "Archived 2 messages (undoable in Regards)."
        );
    }
}
