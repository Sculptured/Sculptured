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

it("keeps a workflow whose stages have no single agent: Community's Coding hospital example", async () => {
  const { readFileSync } = await import('node:fs');
  const team = JSON.parse(readFileSync(`${process.cwd()}/../examples/community/coding-hospital.json`, 'utf8')).team;
  const group = { id: 'team', label: 'Coding hospital', x: 0, y: 0, w: 2000, h: 1200 };
  const block = { id: 'coding-hospital', name: 'Coding hospital', revision: 0, boundary_notes: [], draft: { ...team, groups: [group], agents: team.agents.map((a: { group_id: string | null }) => ({ ...a, group_id: 'team' })), connections: [] } };
  const out = sanitizeExport(block);
  expect(out.draft.workflow?.stages.length).toBe(team.workflow.stages.length);
  const panel = team.workflow.stages.find((s: { reviewers?: string[] }) => s.reviewers?.length);
  expect(() => sanitizeExport({ ...block, draft: { ...block.draft, workflow: { ...block.draft.workflow, stages: block.draft.workflow.stages.map((s: { id: string }) => (s.id === panel.id ? { ...s, reviewers: ['nobody'] } : s)) } } })).toThrow();
});
