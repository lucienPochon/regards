<!--
DRAFT for the website (plan §6, Phase 7): not published, not linked from the app.

Before publishing:
- Confirm each row on a real account (plan §5.1, "Accepted when"). Until a row is confirmed,
  its "Checked" cell says "Not yet". Only the self-hosted row is checked today, by CI against Dovecot.
- Google: once Google verifies the app, change the Gmail rows to "Sign in with Google".
  Until then the app-password path is what ships (plan §6, Phase 7).
- Keep it in step with docs/privacy.md and the app's own wording.
-->

# Does Regards work with my email?

Regards works with any email account that offers **IMAP** (for reading) and **SMTP** (for sending). Most do. This page says, provider by provider, how you sign in and what to expect, including the limits.

## At a glance

| Your email | How you sign in | Good to know | Checked |
| --- | --- | --- | --- |
| **Gmail** | An app password, made in your Google account (it needs 2-Step Verification). *Sign in with Google* comes once Google has reviewed Regards | Archiving takes a message out of the Inbox; it stays in *All Mail*. Gmail keeps its own copy of what you send, so Regards doesn't add a second one | Not yet |
| **Google Workspace** (Gmail at work or school) | As for Gmail | Your administrator can turn off IMAP or apps like Regards. If you can't sign in, ask them | Not yet |
| **Outlook.com, Hotmail, Live** | *Sign in with Microsoft* in your browser. Microsoft doesn't let these accounts use a password in mail apps | Microsoft keeps its own copy of what you send | Not yet |
| **Microsoft 365** (Outlook at work or school) | *Sign in with Microsoft* | Your organisation may need to approve Regards first; the sign-in page says so. Some organisations turn off sending from mail apps (SMTP AUTH): reading still works, and sending fails with Microsoft's reason. Only your administrator can turn it back on | Not yet |
| **iCloud Mail** | An app-specific password, made at appleid.apple.com (it needs two-factor authentication) | | Not yet |
| **Yahoo Mail, AOL** | An app password, made in your account's security settings | | Not yet |
| **Fastmail** | An app password, made in Fastmail's settings | | Not yet |
| **Infomaniak** | Your mailbox password | | Not yet |
| **GMX, WEB.DE** | Your password | Turn on IMAP in their settings first; they keep mail apps out until you do | Not yet |
| **Bluewin** | Your password | | Not yet |
| **Your own server, or your web host's** | Your mailbox password. Regards finds the servers from your address (DNS) or you type them in | The server must use TLS with a valid certificate. Regards never connects without checking it | Yes, in every build's tests |

**Not in Regards 1.0:**
- **Exchange servers without IMAP** (Exchange Web Services only).
- **Proton Mail.** It offers IMAP only through Proton Mail Bridge, which we haven't tested yet.
- **Tuta.** It offers no IMAP.

## The same everywhere

These don't depend on your provider: they happen on your computer.
- **New mail** shows up as it arrives, on servers that support IMAP IDLE, which most do; others are checked every 5 minutes. Your other folders (Sent from your phone, folders your server files mail into) follow within 5 minutes.
- **Smart Inbox, snooze, reminders, pins, search, templates and signatures** all work, offline too, with mail you already have.
- **Undo send:** every message waits 10 seconds before it goes, so you can take it back. You can set 5 seconds to a minute.
- **No internet:** what you send waits in the Outbox, and goes when you're back online.

## Scheduled mail needs your computer on (until Regards 1.1)

You can schedule a message for later with any provider. In Regards 1.0, **your computer must be on at that time**, with Regards running in the background, which is on by default. The Outbox says so beside each scheduled message, for example: "This computer must be on then."

If your computer was off or asleep:
- **Up to an hour late:** the message goes as soon as Regards is back.
- **More than an hour late:** it waits for you in the Outbox, and you choose *Send now* or edit it. A "Monday 9:00" message may not suit Monday at 15:00, so Regards doesn't send it on its own.

**From Regards 1.1:** your provider can hold the message where it supports that. A Pro option will send it on time with your computer off, whatever your provider.

## Your passwords and sign-ins

Passwords, app passwords and sign-in tokens are kept in your computer's own credential store: Windows Credential Manager, the macOS Keychain or the Linux Secret Service. They never go to us. Regards runs no servers and collects no data. See the [privacy statement](../privacy.md).

## AI, with your own key

Regards can summarise a conversation or draft a reply, but only when you ask, and only with an AI provider you choose: Anthropic with your own key, or any OpenAI-compatible server, including one on your own computer. It is off until you turn it on. Before anything is sent, Regards shows how much would go and where. Messages with sign-in or verification codes are never sent. This works the same whatever your email provider.
