# ADR-0016: Version 1 connects through IMAP/SMTP with passwords only; OAuth providers come later

- **Status:** **Superseded by [ADR-0018](0018-modern-minimal-v1.md)** (2026-10-18). 1.0 is OAuth-first IMAP, not password-only. Kept for the record.
- **Date:** 2026-10-18
- **Deciders:** Project owner
- **Amends:** [ADR-0009](0009-mail-protocol-stack.md) (adapter order and timing)

## Context

The project owner decided that **Regards 1.0 supports IMAP only**, and that Google, Microsoft and other provider-specific integrations come later.

Two facts shape what "IMAP only" means for users, as of 2026-10-18:

- **Microsoft:** basic authentication (password or app password) for IMAP, POP and SMTP has been **disabled for personal Outlook.com accounts since 16 September 2024**, and for Exchange Online / Microsoft 365. Only OAuth 2.0 works.
- **Google:** Gmail still accepts IMAP/SMTP with an **app password** when the account has 2-Step Verification. Google Workspace admins can disable this, and Google describes app passwords as a legacy path. Regular account passwords are not accepted.

OAuth for Google's full-mail scope is a *restricted* scope. It needs app verification, and an annual CASA security assessment when data passes through our servers ([06](../research/06-costs-and-business-model.md)).

## Decision

1. **Regards 1.0** ships one provider adapter: **generic IMAP + SMTP**.
   - Authentication: **password or app password only** (SASL PLAIN/LOGIN over TLS). There is no OAuth in 1.0.
   - The full protocol feature set of ADR-0009 still applies: QRESYNC, IDLE, MOVE/UIDPLUS, SPECIAL-USE, X-GM-EXT-1 when present, the async stack on rustls, and the provider trait.
2. **Setup UX in 1.0:**
   - Autoconfiguration (ISPDB-style, DNS SRV) plus manual entry.
   - Built-in, plain-language guidance for app passwords at common providers: Gmail, iCloud, Yahoo, Fastmail, and others that need them.
   - **Outlook.com / Microsoft 365 addresses are detected and the user is told clearly** that they are not supported until a later version. We never suggest workarounds that weaken account security.
3. **Order after 1.0** (each step is a separate release, with its own ADR when started):
   1. **Microsoft OAuth (XOAUTH2 over IMAP/SMTP)**. It costs nothing (a free Entra app registration and publisher verification) and brings back Outlook.com and Microsoft 365 users.
   2. **Microsoft Graph adapter**, for tenants that disable IMAP/SMTP.
   3. **Google OAuth (XOAUTH2)** after Google app verification. CASA is not expected while all data stays local; confirm this with Google first.
   4. **Gmail API adapter**, then **JMAP**.
4. **Architecture is unchanged.** The provider trait and capability model are still built in 1.0, so OAuth providers slot in later without schema or API redesign. OAuth token storage in the vault is designed in Phase 0 but not shipped.

## Alternatives considered

- **Include Microsoft OAuth in 1.0.** It costs no money, only engineering time, and gains the large Outlook.com user base. Rejected for 1.0 to keep scope focused; it is the **first** post-1.0 item.
- **Include Google OAuth in 1.0.** Requires Google verification (time-consuming), and app passwords already cover many Gmail users. Deferred.
- **Support Outlook.com through app passwords.** Not possible: Microsoft rejects basic authentication.

## Consequences

- **v1 costs fall to almost nothing:** no Google verification, no CASA, no Microsoft registration, and no paid test tenants needed. Free consumer accounts and local Dovecot fixtures are enough ([06 §1a](../research/06-costs-and-business-model.md)).
- **Send even when Regards is off** is part of 1.0 through background mode, SMTP FUTURERELEASE and the self-hosted **Regards Relay** ([ADR-0017](0017-send-when-off-relay-in-v1.md)).
- **v1 audience:** users of Gmail (with app passwords), iCloud, Yahoo, Fastmail, GMX/Web.de, mailbox.org, Proton (via Proton Mail Bridge), hosting-provider mailboxes and self-hosted servers. **Not** Outlook.com or Microsoft 365 users. This must be stated on the website and in setup.
- Gmail users whose Workspace admin disables app passwords cannot use 1.0.
- Plan phases are updated: Microsoft Graph moves from Phase 2 to after 1.0 ([03 §6](../research/03-independent-implementation-plan.md)).

## References

- [Outlook.com basic-auth shutdown (vendor note)](https://bluemail.me/app-password/outlook/); [Microsoft Q&A: basic authentication disabled](https://learn.microsoft.com/en-us/answers/questions/5649919/cannot-sign-in-programmatically-with-app-password)
- [Gmail app passwords in 2026 (guide)](https://www.codewords.ai/blog/imap-password-for-gmail)
- [ADR-0009](0009-mail-protocol-stack.md)
