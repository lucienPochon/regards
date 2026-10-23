# OAuth sign-in: registering the apps

Regards signs in to Microsoft and Google accounts with OAuth 2.0, using the authorization code flow with PKCE, through the system browser and a loopback redirect ([ADR-0018](adr/0018-modern-minimal-v1.md)). Each provider needs an app registered with it, which gives a **client id**. A build without one doesn't offer that provider's sign-in, and its accounts use an app password instead.

This page is for maintainers. People using Regards never see any of it.

## Where Regards gets the client ids

The daemon reads them at start-up from the environment. Failing that, it uses the ones the build was made with (`option_env!`, read at compile time):

| Variable | What |
| --- | --- |
| `REGARDS_MICROSOFT_CLIENT_ID` | The Entra app's *Application (client) ID* |
| `REGARDS_GOOGLE_CLIENT_ID` | The Google OAuth client's id |
| `REGARDS_GOOGLE_CLIENT_SECRET` | The secret Google gives a *Desktop app* client. It is not confidential: it ships in every copy, as Google expects for installed apps. PKCE is what protects the sign-in |

- **Release builds:** the release workflow reads repository secrets of the same names (*Settings* → *Secrets and variables* → *Actions*). A missing one gives a warning in the run, and that build has no sign-in for the provider.
- **Development:** set them in the shell that runs `npm run tauri dev`. The app starts the daemon, which inherits them.

None of these are secrets of the person using Regards. Their tokens are separate:
- **Refresh tokens:** kept only in the OS credential store, under `{account id}/oauth-refresh`.
- **Access tokens:** kept only in the daemon's memory.

## Microsoft (Outlook.com, Hotmail, Microsoft 365)

1. In the [Microsoft Entra admin center](https://entra.microsoft.com), go to *App registrations* → *New registration*.
   - **Supported account types:** accounts in any organizational directory *and* personal Microsoft accounts. Regards uses the `common` endpoint.
   - **Redirect URI:** platform *Public client/native (mobile & desktop)*, value `http://localhost`. Microsoft accepts any port on a localhost redirect for native apps; Regards picks a free one for each sign-in.
2. Add no client secret or certificate. Regards is a public client.
3. **API permissions** (delegated): `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`, `openid`, `email`. Regards asks for them at sign-in, as `https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email`.
4. **Publisher verification** (free, with a Microsoft partner ID). Without it, people outside the registering tenant see an "unverified" warning, and many organisations block consent.
5. Copy the *Application (client) ID* into `REGARDS_MICROSOFT_CLIENT_ID`.

**Known limits:**
- Microsoft 365 tenants can turn off SMTP AUTH, per mailbox or for the whole tenant. Sending then fails with the server's reason (for example `5.7.139 … SmtpClientAuthentication is disabled`), and receiving still works. Only the tenant's admin can turn it back on.
- Some tenants require admin consent for third-party apps. The browser says so during sign-in.

## Google (Gmail, Google Workspace)

1. In the [Google Cloud console](https://console.cloud.google.com), create a project, then set up the *OAuth consent screen* (in newer consoles, *Google Auth Platform* → *Branding*, *Audience*, *Data access*):
   - **User type:** external.
   - **Scopes:** `https://mail.google.com/`, `openid`, `email`.
2. Create an *OAuth client ID* of type **Desktop app**. Google allows loopback redirects to `127.0.0.1` on any port for this type. Copy the id and the secret into the variables above.
3. **Verification.** `https://mail.google.com/` is a *restricted* scope. Until Google verifies the app, it stays in *Testing*:
   - it works only for the test users listed on the consent screen, at most 100;
   - Google ends its refresh tokens after **7 days**, so those accounts show *Sign in again* every week. That is expected in Testing.

   For the timeline, and the question of whether the CASA assessment applies to a local-only client, see the [plan](research/03-independent-implementation-plan.md), Phase 0, and [costs](research/06-costs-and-business-model.md).

Until Google approves the app, Gmail users add their account with an **app password**: imap.gmail.com 993 (TLS) and smtp.gmail.com 587 (STARTTLS).

## What the sign-in does

1. `account.oauth_start`:
   - The daemon listens on `127.0.0.1` on a port the system picks.
   - It makes a PKCE verifier and a random `state`.
   - It answers with the provider's page, which the app opens in the system browser.
2. The person signs in. The provider sends the browser back to the loopback address with a code. Only a redirect that carries the sign-in's `state` is accepted.
3. `account.oauth_finish`:
   1. The daemon trades the code (with the PKCE verifier) for tokens.
   2. It reads the address and name from the ID token.
   3. It checks IMAP with the access token (SASL XOAUTH2).
   4. It keeps the refresh token in the credential store.
   5. It adds the account. If an account with that address already signs in this way, it renews that account's sign-in instead.
4. **After that:**
   - IMAP and SMTP sign in with the access token, which is renewed two minutes before it expires, or once when a server turns it down.
   - If the provider refuses the refresh token (revoked, password changed, Testing's 7 days), the account shows `needs_sign_in`, and nothing asks the provider again until the person signs in anew.
