# Using Regards with AI agents

Agents such as Claude Code, Codex and Cursor can read, search, organise and draft your mail through Regards. They see only what you allow, and they never send anything without you: you approve each message, or, for an account where you choose so, you get time to call it back.

## 1. Choose what agents may do

In Regards, open **Settings › Agents** and set each account:

| Level | Agents may |
| --- | --- |
| Off (default) | Nothing: they do not see the account |
| Read | List, search and read mail. Reading never marks it read |
| Triage | Read, and organise (archive, move, flag, pin, snooze, sort senders) and write drafts |
| Send | Triage, and ask to send |

Only you can change these, in the app. Agents cannot change them.

## 2. What waits for you

- **Every send**, unless you chose an undo window for the account:
  - **By default:** an agent's `send` waits in Settings › Agents. You see the draft there, and approve or reject it. Regards sends exactly what you saw: if the draft changes after it was held, it is not sent on that approval.
  - **With an undo window** (Settings › Agents, per account, 10 seconds to 10 minutes): the message waits in the Outbox for that long, and you are told. You can call it back there before it goes; the agent cannot. More than 20 such sends in an hour wait for your approval again.
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
