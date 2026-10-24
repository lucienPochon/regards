import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path }));
const { AccountCheckError, addAccount, providerFor, setLogin } = await import("./accounts");

describe("providerFor", () => {
  it("offers the provider's sign-in for its own addresses", () => {
    expect(providerFor("Ada@Hotmail.com")).toBe("microsoft");
    expect(providerFor("ada@outlook.fr")).toBe("microsoft");
    expect(providerFor("ada@gmail.com")).toBe("google");
  });

  it("knows nothing of other domains or of text that is no address", () => {
    expect(providerFor("ada@infomaniak.ch")).toBeNull();
    expect(providerFor("gmail.com")).toBeNull();
    expect(providerFor("")).toBeNull();
  });
});

describe("a refused account", () => {
  const input = {
    displayName: "Ada",
    email: "ada@example.org",
    username: null,
    password: "pw",
    imapHost: "imap.example.org",
    imapPort: 993,
    smtpHost: "smtp.example.org",
    smtpPort: 587,
    smtpSecurity: "starttls" as const,
  };

  it("says what the server refused, and which user names were tried", async () => {
    const check = { reason: "auth_failed", server: "imap", tried: ["ada@example.org", "ada"] };
    invoke.mockRejectedValueOnce({ message: "the server refused the user name or password: no", check });
    const failure = await addAccount(input).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AccountCheckError);
    expect((failure as InstanceType<typeof AccountCheckError>).check).toEqual(check);
    // Forms that show String(failure) keep showing the message alone.
    expect(String(failure)).toBe("the server refused the user name or password: no");
  });

  it("has no check when no server was asked", async () => {
    invoke.mockRejectedValueOnce("Regards is quitting");
    const failure = await setLogin({ ...input, id: "a1" }).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AccountCheckError);
    expect((failure as InstanceType<typeof AccountCheckError>).check).toBeNull();
    expect(String(failure)).toBe("Regards is quitting");
  });
});
