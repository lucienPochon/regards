import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const {
  AGENT_ACCESS_LEVELS,
  AGENT_APPS,
  activityKind,
  agentAppsFound,
  approveHold,
  isConnected,
  isFound,
  listAgentActivity,
  listHolds,
  rejectHold,
  setAgentAccess,
} = await import("./agents");

describe("agent apps", () => {
  const app = (id: string) => AGENT_APPS.find((entry) => entry.id === id)!;
  const settings = { enabled: true, connected: 1, apps: ["codex-mcp-client"] };
  const found = [
    { id: "claude-code" as const, set_up: true },
    { id: "cursor" as const, set_up: false },
  ];

  it("are looked for by the app", async () => {
    invoke.mockResolvedValueOnce(found);
    expect(await agentAppsFound()).toEqual(found);
    expect(invoke).toHaveBeenLastCalledWith("agent_apps_found");
  });

  it("are found on this computer, or by being connected now", () => {
    expect(isFound(app("claude-code"), null, found)).toBe(true);
    expect(isFound(app("codex"), settings, found)).toBe(true);
    expect(isFound(app("claude-app"), settings, found)).toBe(false);
  });

  it("are connected once Regards is set up in them, or while connected now", () => {
    expect(isConnected(app("claude-code"), null, found)).toBe(true);
    expect(isConnected(app("cursor"), null, found)).toBe(false);
    expect(isConnected(app("codex"), settings, null)).toBe(true);
    expect(isConnected(app("claude-app"), settings, found)).toBe(false);
  });
});

describe("agent access", () => {
  it("is set through the app, and answers with what was stored", async () => {
    invoke.mockResolvedValueOnce({ agent_access: "read" });
    expect(await setAgentAccess("a1", "read")).toBe("read");
    expect(invoke).toHaveBeenLastCalledWith("account_set_agent_access", { accountId: "a1", access: "read" });
  });

  it("lists what agents did, all accounts by default", async () => {
    invoke.mockResolvedValueOnce({ entries: [], next: null });
    await listAgentActivity();
    expect(invoke).toHaveBeenLastCalledWith("agent_activity", { accountId: null, before: null, limit: null });
    invoke.mockResolvedValueOnce({ entries: [], next: null });
    await listAgentActivity({ accountId: "a1", before: 40, limit: 20 });
    expect(invoke).toHaveBeenLastCalledWith("agent_activity", { accountId: "a1", before: 40, limit: 20 });
  });

  it("lists what waits for the person, and decides it", async () => {
    invoke.mockResolvedValueOnce({ holds: [] });
    expect(await listHolds()).toEqual([]);
    expect(invoke).toHaveBeenLastCalledWith("hold_list", { pending: true });
    invoke.mockResolvedValueOnce({ id: 3, status: "approved" });
    await approveHold(3);
    expect(invoke).toHaveBeenLastCalledWith("hold_approve", { id: 3 });
    invoke.mockResolvedValueOnce({ id: 4, status: "rejected" });
    await rejectHold(4);
    expect(invoke).toHaveBeenLastCalledWith("hold_reject", { id: 4 });
  });

  it("lists the levels from none to the most", () => {
    expect(AGENT_ACCESS_LEVELS.map((level) => level.value)).toEqual(["off", "read", "triage", "send"]);
  });
});

describe("agent activity", () => {
  it("is reading, sorting or sending, by what was asked", () => {
    const kinds = Object.fromEntries(
      ["message.get", "search.query", "draft.get", "message.archive", "sender.block", "draft.reply", "draft.send", "hold.get", "account.set_agent_access"].map(
        (method) => [method, activityKind(method)],
      ),
    );
    expect(kinds).toEqual({
      "message.get": "reading",
      "search.query": "reading",
      "draft.get": "reading",
      "message.archive": "sorting",
      "sender.block": "sorting",
      "draft.reply": "sending",
      "draft.send": "sending",
      "hold.get": null,
      "account.set_agent_access": null,
    });
  });
});
