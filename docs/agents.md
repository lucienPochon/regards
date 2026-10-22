# Using Regards with AI agents

Agents such as Claude Code, Codex and Cursor can read, search, organise and draft your mail through Regards. They see only what you allow, and they never send anything without you: you approve each message, or, for an account where you choose so, you get time to call it back.

## 1. Choose what agents may do

In Regards, open **Settings › Agents** and set each account:

| Level | Agents may |
| --- | --- |
| Off (default) | Nothing: they do not see the account |
| Read | List, search and read mail, your voice and notes about people. Reading never marks it read |
| Triage | Read, and organise (archive, move, flag, pin, snooze, sort senders), write drafts, keep notes about people and suggest a voice |
| Send | Triage, and ask to send |

Only you can change these, in the app. Agents cannot change them.

To stop every agent at once, turn agents off in Settings › Agents. While they are off, agents get nothing, whatever each account allows: they are told "agents are turned off in Regards". The agents connected at that moment are disconnected. Each account keeps its level for when you turn agents on again. You can also disconnect the agents connected now without turning them off, and they may connect again.

## 2. What waits for you

- **Every send**, unless you chose an undo window for the account:
  - **By default:** an agent's `send` waits in Settings › Agents. You see the draft there, and approve or reject it. Regards sends exactly what you saw: if the draft changes after it was held, it is not sent on that approval.
  - **With an undo window** (Settings › Agents, per account, 10 seconds to 10 minutes): the message waits in the Outbox for that long, and you are told. You can call it back there before it goes; the agent cannot. More than 20 such sends in an hour wait for your approval again.
- **Changing more than 50 messages at once.**
- **Deleting for good.**
- **A suggested voice**: you see it, and approve it, edit it first, or reject it.
- **A note with a link, an email address or bank details**, with a warning when it comes from a message that person did not send.

You get a notification. A request you do not decide within a day expires.

## 3. See and undo what agents did

Settings › Agents lists every request an agent made, allowed or refused, with what it changed. Changes can be undone there for ten minutes.

## 4. Connect your agent

The easiest way is in Regards: **Connect** beside an agent in the Agents menu, or **Settings › Agents › Connect an agent…**, opens a dialog with a card per agent and its steps:

- **Claude Code:** a request to send it (the dialog can open Claude Code with it ready). Claude Code runs `regards skill` and installs the skill.
- **Claude app:** add Regards to its settings file, then restart it.
- **Cursor:** **Add to Cursor** asks Cursor to add Regards as an MCP server; you confirm there.
- **Codex:** run `codex mcp add regards -- regards mcp`.
- **Other agents:** the same request as Claude Code, or `regards mcp` as an MCP server.

The tab **On this computer** lists the agents found here; each says **Active** once Regards is set up in it. The dialog checks again every few seconds, so you see it turn active.

The `regards` command comes with Regards. For agents to run it from any terminal, it must be on your PATH:

- **Windows:** the Connect dialog offers to put its folder on your Path when it isn't there yet. You can also run `regards path add` from that folder; `regards path remove` takes it off, and uninstalling Regards does too. Terminals and apps opened afterwards find it: restart Claude Code, Cursor or your terminal if they were already open.
- **macOS and Linux:** `regards path` says whether it is found, and otherwise how to link it into a folder on your PATH (for example `sudo ln -sf … /usr/local/bin/regards`).

Agents use it in either of two ways.

**As an MCP server** (`regards mcp`):

- Claude Code: `claude mcp add --scope user regards -- regards mcp`. With `--scope user`, Claude Code has Regards in every folder you start it from.
- The Claude app: Settings › Developer › Edit Config, add the lines below to the file, then restart the app:

  ```json
  { "mcpServers": { "regards": { "command": "regards", "args": ["mcp"] } } }
  ```

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

## 5. Ask your agent from Regards

The **Agents** button at the foot of the sidebar opens two tabs:

- **Recipes:** ready-made requests, such as a morning briefing, the messages that still need your reply, what you promised, or learning how you write. Choose one, then:
  - **Claude Code** opens it in a new terminal, ready: press Enter there to start. This works once you have used Claude Code on this computer at least once.
  - **Claude** opens a new chat in the Claude app, ready to send.
  - **Copy to the clipboard**, for any other agent.

  Nothing is sent until you send it in the agent. Regards itself sends nothing to any AI.
- **Agents:** the agent apps Regards found on this computer, by their settings folders (`~/.claude`, `~/.codex`, `~/.cursor`, the Claude app's). One is marked **Active** once Regards is set up in it (the skill from `regards skill --install` is in its skills folder, or `regards` is one of its MCP servers) or while it is connected; otherwise it has **Connect**, which opens the Connect dialog on its card (§ 4). Regards only looks: it changes nothing in those folders. When it finds none, it lists them all.

You can hide the button from its menu; Settings › Agents shows it again.

## 6. Your voice, and notes about people

So that agents write like you, and remember who people are:

- **Your voice** (Settings › Agents › Your voice): for each of your addresses, a few lines on how you write: language, tone, how formal, how you open and close, what never to promise. Switch it on per address. Agents read it before they draft for you; only you can change it.
  - **To have it learned,** ask your agent: *"Learn how I write from my sent mail at you@example.org and suggest it as my voice in Regards."* It reads your sent messages through Regards, and its suggestion waits for you to approve, edit or reject. Regards itself sends nothing to any AI for this.
- **Notes about people** (Settings › Agents › Notes about people): short facts, by you or an agent, such as who decides or what someone prefers. You see them on each person's card, where you can add, edit or remove them. Agents get them with the mail they read. Switch them off and nobody gets them; **Forget them all** removes them.

Agents get both with every message, conversation or draft, as `context`, or for one person with `regards memory <address>`.

## Safety

- Message text reaches agents as data, with a reminder not to follow instructions found in it.
- Your voice is your guidance; notes about people reach agents as information, never as instructions.
- Agents cannot read your passwords or change settings.
- Regards runs on your computer. Agents talk to it there, not through any server of ours.
