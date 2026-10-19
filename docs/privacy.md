# Privacy

**Regards 1.0 runs no servers and collects no data.** Your mail, your accounts and what you do in Regards stay on your computer. Nobody at Regards can see them, because nothing is ever sent to us.

This statement covers the Regards desktop app, the `regards` command-line tool and the agent interface (MCP) that comes with it.

## What Regards connects to

Regards makes these connections, and no others:

| To | When | What for |
| --- | --- | --- |
| **Your mail servers** (IMAP and SMTP), as you set them up | While Regards runs | Fetching and organising your mail, sending what you send, searching on the server when you ask |
| **Microsoft or Google sign-in**, if you add an account that way | When you sign in, and about once an hour after | Signing in, then renewing the sign-in. Your password goes to Microsoft or Google in your browser, never to Regards |
| **Web servers named in a message**, only if you ask | When you choose *Load images* for a message | Showing that message's remote images. They can tell the sender you opened it, which is why they stay blocked until you ask. No referrer is sent |
| **Web pages you open from a message** | When you click a link | Links open in your own browser |

Every connection to a mail server or a sign-in service uses TLS, and the certificate is always checked. There is no setting to turn that off.

**Not included:**
- analytics, telemetry or crash reporting;
- advertising;
- fonts or scripts fetched from the internet;
- "phone home" checks.

Regards does not check for updates yet. When it does, this statement will say exactly what that check sends.

## What Regards keeps on your computer

- **Your mail:** messages, folders, drafts, the search index and attachments you downloaded, in Regards' data folder on your computer.
- **Passwords and sign-in tokens:** only in your operating system's credential store (Windows Credential Manager, the macOS Keychain, or the Secret Service on Linux). They never go into Regards' database or logs.
- **Settings and history:** your settings, the Outbox, and a record of what agents asked (kept 90 days), in the same folder.
- **A log:** Regards' own log, for diagnosing problems. It never contains message content, passwords or tokens. It is kept small: one older copy at most.

Regards relies on your operating system's disk encryption to protect these files. To remove everything, remove your accounts in Regards, then delete its data folder.

## What your sent mail reveals

Regards adds nothing that identifies your computer:
- A message's ID is random, on your address's domain, never your computer's name.
- Its date is in UTC, so it doesn't reveal your time zone.
- There is no "sent with" header.
- Bcc recipients never appear in the message itself.

When connecting to send, Regards doesn't tell the server your computer's name either.

## Agents

AI agents connected through the `regards` tool or MCP:
- **See only accounts you open to them,** in Settings › Agents. Every account is closed to them until you open it.
- **Talk to Regards on your computer only,** over a local connection. Regards sends nothing to the agent's company.

What an agent does with what it reads is up to that agent and its provider. Choose agents you trust with that account's mail.

## Google user data

Regards' use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.

Mail from a Google account is used only to show it to you and to do what you ask, on your computer. It is not sent to Regards or to anyone else. It is not used for advertising, and nobody reads it.

## Changes

If anything here changes, for example when Regards 1.1 offers optional sending while your computer is off, this statement will be updated first. New features that send data anywhere will be off until you turn them on.
