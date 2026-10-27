// @vitest-environment jsdom
// A menu rendered: a switch in it flips and the menu stays; any other pick closes it.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const { Menu } = await import("./Menu");
const { snoozeAlert, timeOptionEntry } = await import("./snooze");

afterEach(cleanup);

describe("a switch in a menu", () => {
  it("flips when picked, and the menu stays open", () => {
    const onClose = vi.fn();
    const later = vi.fn();
    const alert = snoozeAlert();
    const show = vi.fn();
    const request = { anchor: { x: 0, y: 0 }, label: "Snooze until", entries: [{ label: "Later today", onSelect: later }, timeOptionEntry(alert, show)] };
    render(<Menu request={request} onClose={onClose} />);

    const toggle = screen.getByRole("menuitemcheckbox", { name: "Alert me when it comes back" });
    expect(toggle.getAttribute("aria-checked")).toBe("true");
    fireEvent.click(toggle);
    expect(alert.on).toBe(false);
    expect(show).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("menuitem", { name: "Later today" }));
    expect(later).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
