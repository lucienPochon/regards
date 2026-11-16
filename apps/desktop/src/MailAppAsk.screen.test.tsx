// @vitest-environment jsdom
// "Make Regards your default mail app?", asked once in the main window: only
// when it is not already, never again once answered, and "Done" only once
// Windows really opens mailto: links in Regards.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let status = { registered: false, can_register: true };
const invoke = vi.fn((command: string) => {
  switch (command) {
    case "mail_app_status":
      return Promise.resolve(status);
    case "make_mail_app":
      return Promise.resolve(null);
    default:
      return Promise.resolve(null);
  }
});
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { AskMailApp } = await import("./MailAppAsk");
const { getPrefs, setPrefs } = await import("./prefs");

const QUESTION = "Make Regards your default mail app?";
const onPlatform = (platform: string) => Object.defineProperty(navigator, "platform", { value: platform, configurable: true });

beforeEach(() => {
  status = { registered: false, can_register: true };
  setPrefs({ mailAppAsked: false });
  onPlatform("Win32");
  invoke.mockClear();
});
afterEach(cleanup);

describe("asking to be the default mail app", () => {
  it("asks when Regards is not the mail app yet", async () => {
    render(<AskMailApp hidden={false} />);
    expect(await screen.findByText(QUESTION)).toBeTruthy();
    expect(screen.getByText("Email addresses you click elsewhere will open here.")).toBeTruthy();
  });

  it("does not ask when it already is", async () => {
    status = { registered: true, can_register: true };
    const { container } = render(<AskMailApp hidden={false} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("mail_app_status"));
    expect(container.textContent).toBe("");
  });

  it("never asks again once answered", () => {
    setPrefs({ mailAppAsked: true });
    const { container } = render(<AskMailApp hidden={false} />);
    expect(container.textContent).toBe("");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("says where to find it later after Not now", async () => {
    render(<AskMailApp hidden={false} />);
    fireEvent.click(await screen.findByRole("button", { name: "Not now" }));
    expect(getPrefs().mailAppAsked).toBe(true);
    expect(screen.getByText("OK. It’s in Settings › General if you change your mind.")).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("make_mail_app");
  });

  it("on Windows, waits for the choice in Default apps, and says Done only once it took", async () => {
    render(<AskMailApp hidden={false} />);
    fireEvent.click(await screen.findByRole("button", { name: "Yes" }));
    expect(getPrefs().mailAppAsked).toBe(true);
    expect(await screen.findByText("In Default apps, which just opened, choose Regards for MAILTO.")).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith("make_mail_app");

    // Back without choosing it: still waiting.
    await act(async () => void window.dispatchEvent(new Event("focus")));
    expect(screen.queryByText("Done: Regards is your default mail app.")).toBeNull();

    status = { registered: true, can_register: true };
    await act(async () => void window.dispatchEvent(new Event("focus")));
    expect(await screen.findByText("Done: Regards is your default mail app.")).toBeTruthy();
  });

  it("on a Mac, says where Mail chooses it", async () => {
    onPlatform("MacIntel");
    status = { registered: false, can_register: false };
    const { container } = render(<AskMailApp hidden={false} />);
    expect(await screen.findByText("Choose it in Mail, under Settings › General.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(getPrefs().mailAppAsked).toBe(true);
    expect(container.textContent).toBe("");
    expect(invoke).not.toHaveBeenCalledWith("make_mail_app");
  });

  it("stays out of the way of a message written full size", async () => {
    const { container, rerender } = render(<AskMailApp hidden />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("mail_app_status"));
    expect(container.textContent).toBe("");
    rerender(<AskMailApp hidden={false} />);
    expect(await screen.findByText(QUESTION)).toBeTruthy();
  });
});
