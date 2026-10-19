import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const { AGENT_ACCESS_LEVELS, approveHold, listAgentActivity, listHolds, rejectHold, setAgentAccess } = await import(
  "./agents"
);

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
