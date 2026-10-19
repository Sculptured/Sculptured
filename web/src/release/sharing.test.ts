import { describe, it, expect } from "vitest";
import { emptyBlock } from "../groups/blocks";
import { sanitizeExport } from "./sharing";
describe("Community export boundary", () => {
  it("removes local inputs and permissions without mutating the private original", () => {
    const b = emptyBlock();
    b.draft.agents.push({
      id: "agent",
      name: "Agent",
      kind: "codex",
      job: "Work",
      folder: null,
      group_id: "group",
      x: 0,
      y: 0,
      advanced: {
        model: null,
        permission: "unrestricted",
        resume: "resume-if-possible",
      },
    });
    b.draft.folder = "/private/project";
    b.draft.agents[0].folder = "/private/agent";
    b.boundary_notes = ["private"];
    const c = sanitizeExport(b);
    expect(c.draft.folder).toBeUndefined();
    expect(c.draft.agents[0].folder).toBeNull();
    expect(c.draft.agents[0].advanced.permission).toBe("restricted");
    expect(c.boundary_notes).toEqual([]);
    expect(b.draft.folder).toBe("/private/project");
  });
  it("rejects a project-shaped upload", () => {
    expect(() =>
      sanitizeExport({ draft: { groups: [], agents: [] } } as never),
    ).toThrow("group export");
  });
});

describe('a Library copy from Community', () => {
  it('drops where it came from when exported or published', () => {
    const b = { ...emptyBlock(), source: { community_block: 'b1', version: 1 } };
    expect(sanitizeExport(b).source).toBeUndefined();
  });
});
