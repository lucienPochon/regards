# Using Regards with AI agents

Agents such as Claude Code, Codex and Cursor can read, search, organise and draft your mail through Regards. They see only what you allow, and they never send anything without you.

## 1. Choose what agents may do

In Regards, open **Settings › Agents** and set each account:

| Level | Agents may |
| --- | --- |
| Off (default) | Nothing: they do not see the account |
| Read | List, search and read mail. Reading never marks it read |
| Triage | Read, and organise (archive, move, flag, pin, snooze, sort senders) and write drafts |
| Send | Triage, and ask to send |

Only you can change these, in the app. Agents cannot change them.

## 2. What always waits for you

- **Every send.** An agent's `send` waits in Settings › Agents, where you see the draft and approve or reject it. Regards sends exactly what you saw: if the draft changes after it was held, it is not sent on that approval.
- **Changing more than 50 messages at once.**
- **Deleting for good.**

You get a notification. A request you do not decide within a day expires.

## 3. See and undo what agents did

Settings › Agents lists every request an agent made, allowed or refused, with what it changed. Changes can be undone there for ten minutes.

## 4. Connect your agent

The `regards` command comes with Regards. Agents use it in either of two ways.

**As an MCP server** (`regards mcp`):

- Claude Code: `claude mcp add regards -- regards mcp`
- Codex, in `~/.codex/config.toml`:

  ```toml
  [mcp_servers.regards]
  command = "regards"
  args = ["mcp"]
  ```

- Cursor, in `~/.cursor/mcp.json`:

  ```json
  { "mcpServers": { "regards": { "command": "regards", "args": ["mcp"] } } }
  ```

**As commands, with a skill that explains them:**

```sh
regards skill --install ~/.claude/skills   # writes ~/.claude/skills/regards/SKILL.md
regards search "invoice from:shop" --json  # every command prints JSON with --json
```

`regards --help` lists the commands. `regards <command> --help` says what each needs and whether it changes mail.

## Safety

- Message text reaches agents as data, with a reminder not to follow instructions found in it.
- Agents cannot read your passwords or change settings.
- Regards runs on your computer. Agents talk to it there, not through any server of ours.
