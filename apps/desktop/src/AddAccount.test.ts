import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: (path: string) => path }));
const { fill, guessName } = await import("./AddAccount");

const blank = {
  displayName: "",
  email: "ada@example.org",
  username: null,
  password: "",
  imapHost: "",
  imapPort: 993,
  smtpHost: "",
  smtpPort: 587,
  smtpSecurity: "starttls" as const,
};

describe("guessName", () => {
  it("makes a name of the address's words", () => {
    expect(guessName("ada.lovelace@example.org")).toBe("Ada Lovelace");
    expect(guessName("grace_hopper@example.org")).toBe("Grace Hopper");
    expect(guessName("ADA@example.org")).toBe("Ada");
  });

  it("leaves out what is no word", () => {
    expect(guessName("info2024@example.org")).toBe("");
    expect(guessName("j.smith-42@example.org")).toBe("J Smith");
  });
});

describe("fill", () => {
  const found = {
    source: "known" as const,
    imap: { host: "imap.example.org", port: 993, security: "tls" as const },
    smtp: { host: "smtp.example.org", port: 465, security: "tls" as const },
    oauth_only: false,
  };

  it("puts what the lookup found in the fields", () => {
    expect(fill(blank, found, new Set())).toMatchObject({
      imapHost: "imap.example.org",
      imapPort: 993,
      smtpHost: "smtp.example.org",
      smtpPort: 465,
      smtpSecurity: "tls",
    });
  });

  it("leaves the fields the person typed in alone", () => {
    const typed = { ...blank, imapHost: "mail.example.org", smtpPort: 2525 };
    expect(fill(typed, found, new Set(["imapHost", "smtpPort"] as const))).toMatchObject({
      imapHost: "mail.example.org",
      smtpHost: "smtp.example.org",
      smtpPort: 2525,
    });
  });

  it("takes no receiving server that isn't TLS", () => {
    const plain = { ...found, imap: { host: "imap.example.org", port: 143, security: "starttls" as const } };
    expect(fill(blank, plain, new Set())).toMatchObject({ imapHost: "", imapPort: 993 });
  });
});
