// @vitest-environment jsdom
// The sign-up screens rendered, against a daemon that answers as each test sets it up.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path }));
const { AddAccount, AddAccountDialog } = await import("./AddAccount");

const account = (email: string) => ({
  id: "added",
  display_name: "",
  email,
  username: email,
  imap: { host: "mail.infomaniak.test", port: 993, security: "tls" },
  smtp: { host: "mail.infomaniak.test", port: 465, security: "tls" },
  imap_capabilities: ["IMAP4rev1"],
  created_at: 0,
});

/** What discovery says, by domain. */
function discover({ email }: Args) {
  const domain = String(email).split("@")[1];
  if (domain === "infomaniak.test") {
    return {
      source: "known",
      imap: { host: "mail.infomaniak.test", port: 993, security: "tls" },
      smtp: { host: "mail.infomaniak.test", port: 465, security: "tls" },
      oauth_only: false,
      provider_name: "Infomaniak",
    };
  }
  if (domain === "proton.test") {
    return {
      source: "known",
      imap: null,
      smtp: null,
      oauth_only: false,
      provider_name: "Proton Mail",
      no_imap: "Proton Mail lets mail apps in only through its Proton Mail Bridge app.",
    };
  }
  return {
    source: "guess",
    imap: { host: `imap.${domain}`, port: 993, security: "tls" },
    smtp: { host: `smtp.${domain}`, port: 587, security: "starttls" },
    oauth_only: false,
  };
}

let handlers: Record<string, Handler>;

beforeEach(() => {
  handlers = {
    account_oauth_providers: () => ({ providers: ["microsoft", "google"] }),
    account_discover: discover,
    account_check_server: () => ({}),
    account_add: ({ input }) => account((input as { email: string }).email),
    account_update: ({ id }) => ({ ...account("ada@infomaniak.test"), id }),
  };
  invoke.mockImplementation(async (command: string, args: Args = {}) => {
    const handler = handlers[command];
    if (!handler) throw `The test daemon does not know ${command}.`;
    return handler(args);
  });
});

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

const calls = (command: string) => invoke.mock.calls.filter(([name]) => name === command).map(([, args]) => args as Args);
const button = (name: string | RegExp) => screen.getByRole("button", { name }) as HTMLButtonElement;

/** Types the address and leaves the field, which looks it up at once. */
async function enterAddress(address: string, found: RegExp | string) {
  const field = screen.getByLabelText("Email address");
  fireEvent.change(field, { target: { value: address } });
  fireEvent.focusOut(field);
  await screen.findByText(found);
}

/** From the address to the password step, then Sign in. */
async function signIn(password = "a password") {
  await enterAddress("ada@infomaniak.test", "Infomaniak");
  fireEvent.click(button("Continue"));
  fireEvent.change(await screen.findByLabelText("Password"), { target: { value: password } });
  fireEvent.click(button("Sign in"));
}

describe("the address", () => {
  it("leads to the provider's password when its settings are found", async () => {
    render(<AddAccount onDone={vi.fn()} />);
    await enterAddress("ada@infomaniak.test", "Infomaniak");
    expect(screen.getByText(/settings found/)).toBeTruthy();
    fireEvent.click(button("Continue"));
    expect(await screen.findByRole("heading", { name: "Your Infomaniak password" })).toBeTruthy();
    expect(calls("account_discover")).toEqual([{ email: "ada@infomaniak.test" }]);
  });

  it("says when the provider lets no mail app in, and goes no further", async () => {
    render(<AddAccount onDone={vi.fn()} />);
    await enterAddress("ada@proton.test", /Proton Mail Bridge/);
    expect(screen.getByRole("alert").textContent).toContain("Proton Mail Bridge");
    expect(button("Continue").disabled).toBe(true);
  });
});

describe("when signing in fails", () => {
  const cases: { check: Args | null; heading: string; choice: string; anyway: boolean }[] = [
    { check: { reason: "auth_failed", server: "imap", tried: ["ada@infomaniak.test", "ada"] }, heading: "Infomaniak didn't accept the password", choice: "Sign in with another user name", anyway: false },
    { check: { reason: "login_disabled", server: "imap" }, heading: "Infomaniak doesn't take a password here", choice: "Use a password made for Regards", anyway: false },
    { check: { reason: "tls", server: "imap" }, heading: "The connection to mail.infomaniak.test isn't secure", choice: "Check the servers", anyway: false },
    { check: { reason: "connect", server: "imap" }, heading: "mail.infomaniak.test didn't answer", choice: "Check the servers", anyway: false },
    { check: { reason: "timeout", server: "imap" }, heading: "mail.infomaniak.test didn't answer", choice: "Try again", anyway: false },
    { check: { reason: "not_imap", server: "imap" }, heading: "mail.infomaniak.test didn't answer", choice: "Check the servers", anyway: false },
    { check: { reason: "protocol", server: "imap" }, heading: "The account couldn't be added", choice: "Try again", anyway: false },
    { check: { reason: "auth_failed", server: "smtp" }, heading: "The sending server didn't accept the password", choice: "Type the password again", anyway: true },
    { check: { reason: "tls", server: "smtp" }, heading: "Sending isn't encrypted the way Regards needs", choice: "Check the servers", anyway: true },
    { check: { reason: "connect", server: "smtp" }, heading: "The sending server didn't answer", choice: "Check the servers", anyway: true },
    { check: null, heading: "The account couldn't be added", choice: "Try again", anyway: false },
  ];

  it.each(cases)("$check.reason ($check.server): $heading", async ({ check, heading, choice, anyway }) => {
    handlers.account_add = () => {
      throw { message: "[AUTHENTICATIONFAILED] Authentication failed.", check };
    };
    render(<AddAccount onDone={vi.fn()} />);
    await signIn();
    expect(await screen.findByRole("heading", { name: heading })).toBeTruthy();
    expect(button(new RegExp(`^${choice}`))).toBeTruthy();
    expect(screen.queryByRole("button", { name: /^Add it anyway/ }) !== null).toBe(anyway);
    // The server's own words: folded away when a server said them, said outright when none did.
    if (check) expect(screen.getByText("What the server said")).toBeTruthy();
    else expect(screen.getByText("[AUTHENTICATIONFAILED] Authentication failed.")).toBeTruthy();
  });

  it("names the user names tried", async () => {
    handlers.account_add = () => {
      throw { message: "no", check: { reason: "auth_failed", server: "imap", tried: ["ada@infomaniak.test", "ada"] } };
    };
    render(<AddAccount onDone={vi.fn()} />);
    await signIn("wrong");
    await screen.findByRole("heading", { name: "Infomaniak didn't accept the password" });
    expect(screen.getByText("ada@infomaniak.test", { selector: "strong" })).toBeTruthy();
    expect(screen.getByText("ada", { selector: "strong" })).toBeTruthy();
  });

  it("adds it anyway after a sending failure, without checking sending", async () => {
    handlers.account_add = ({ input }) => {
      if (!(input as { skipSendCheck?: boolean }).skipSendCheck) throw { message: "no answer", check: { reason: "connect", server: "smtp" } };
      return account("ada@infomaniak.test");
    };
    render(<AddAccount onDone={vi.fn()} />);
    await signIn();
    fireEvent.click(await screen.findByRole("button", { name: /^Add it anyway/ }));
    expect(await screen.findByRole("heading", { name: "ada@infomaniak.test is ready" })).toBeTruthy();
    expect(screen.getByText("Receiving works. Sending wasn't checked.")).toBeTruthy();
    expect(calls("account_add").map(({ input }) => (input as { skipSendCheck?: boolean }).skipSendCheck)).toEqual([false, true]);
  });
});

describe("the servers form", () => {
  it("checks each server, and offers the port that answers", async () => {
    handlers.account_check_server = ({ server, settings }) => {
      const { host, port } = settings as { host: string; port: number };
      if (host.startsWith("nowhere")) return { reason: "connect" };
      if (server === "smtp" && port === 587) return { reason: "connect", suggestion: { host, port: 465, security: "tls" } };
      return {};
    };
    render(<AddAccount onDone={vi.fn()} />);
    await enterAddress("ada@hosting.test", /Likely settings/);
    fireEvent.click(button("Continue"));
    fireEvent.click(await screen.findByRole("button", { name: "Change servers" }));

    // Filled in already: both are checked on opening.
    fireEvent.click(await screen.findByRole("button", { name: "Use 465" }));
    expect(screen.queryByText(/Nothing answered on port 587/)).toBeNull();
    const [, smtpPort] = screen.getAllByLabelText("Port") as HTMLInputElement[];
    expect(smtpPort.value).toBe("465");
    expect(screen.getByRole("radio", { name: /^TLS/ }).getAttribute("aria-checked")).toBe("true");
    await waitFor(() => expect(screen.getAllByText("It answers, securely")).toHaveLength(2));

    // Leaving a server's fields checks it again.
    const [imapHost] = screen.getAllByLabelText("Server") as HTMLInputElement[];
    fireEvent.change(imapHost, { target: { value: "nowhere.hosting.test" } });
    fireEvent.focusOut(imapHost);
    expect(await screen.findByText("Nothing answered on port 993. Check the server name and port.")).toBeTruthy();
    expect(calls("account_check_server").map(({ server, settings }) => `${server} ${(settings as { host: string }).host}:${(settings as { port: number }).port}`)).toEqual([
      "imap imap.hosting.test:993",
      "smtp smtp.hosting.test:587",
      "imap nowhere.hosting.test:993",
    ]);
  });
});

describe("the Add another account window", () => {
  it("closes with Esc, ✕ or Cancel before anything is saved", async () => {
    const onClose = vi.fn();
    render(<AddAccountDialog onClose={onClose} onAdded={vi.fn()} />);
    expect(screen.getByRole("dialog", { name: "Add another account" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Add another account" })).toBeTruthy();
    // Esc from inside the field too, where the app's own keys stay quiet.
    fireEvent.keyDown(screen.getByLabelText("Email address"), { key: "Escape" });
    fireEvent.click(button("Close"));
    fireEvent.click(button("Cancel"));
    expect(onClose).toHaveBeenCalledTimes(3);
    expect(calls("account_add")).toEqual([]);
  });

  it("stays while signing in, then hands the account over", async () => {
    let release: () => void = () => {};
    handlers.account_add = ({ input }) => new Promise((resolve) => (release = () => resolve(account((input as { email: string }).email))));
    const onClose = vi.fn();
    const onAdded = vi.fn();
    render(<AddAccountDialog onClose={onClose} onAdded={onAdded} />);
    await signIn();

    await screen.findByText("Signing in to receive and send");
    expect(button("Close").disabled).toBe(true);
    fireEvent.keyDown(screen.getByRole("heading", { name: "Connecting ada@infomaniak.test" }), { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();

    release();
    await screen.findByRole("heading", { name: "ada@infomaniak.test is ready" });
    expect((screen.getByLabelText(/^Name people see on your mail/) as HTMLInputElement).value).toBe("Ada");
    fireEvent.click(button("Done"));
    await waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));
    expect(onAdded.mock.calls[0][0]).toMatchObject({ email: "ada@infomaniak.test" });
    expect(calls("account_update")).toEqual([{ id: "added", displayName: "Ada" }]);
    expect(onClose).not.toHaveBeenCalled();
  });
});
