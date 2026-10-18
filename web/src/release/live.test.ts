import { describe, it, expect } from "vitest";
import { compiledRig, seatView, ev } from "../test-fixtures";
import { liveProjection } from "./live";
import { computeLayout } from "../layout";
describe("Live is an evidence view", () => {
  it("omits planned and stopped agents without altering the saved layout", () => {
    const c = compiledRig(),
      layout = computeLayout(c);
    const result = liveProjection(
      c,
      {
        "dev.a@r": seatView("a"),
        "dev.b@r": seatView("b", { lifecycle: "stopped" }),
      },
      [],
      layout,
    );
    expect(result.compiled.seats.map((s) => s.id)).toEqual(["dev.a@r"]);
    expect(result.compiled.edges).toEqual([]);
    expect(result.layout.seats["dev.a@r"]).toEqual(layout.seats["dev.a@r"]);
    expect(layout.seats["dev.b@r"]).toBeDefined();
  });
  it("keeps agents that need you or failed, whose cards carry the reason", () => {
    const c = compiledRig();
    const seats = {
      "dev.a@r": seatView("a", { lifecycle: "attention", lifecycle_reason: "folder missing" }),
      "dev.b@r": seatView("b", { lifecycle: "failed" }),
    };
    expect(liveProjection(c, seats, []).compiled.seats.map((s) => s.id)).toEqual(["dev.a@r", "dev.b@r"]);
  });
  it("creates a route from recorded messages, never from planned edges", () => {
    const c = compiledRig(),
      seats = { "dev.a@r": seatView("a"), "dev.b@r": seatView("b") };
    expect(liveProjection(c, seats, []).compiled.edges).toHaveLength(0);
    const queued = {
      ...ev(
        5,
        {
          type: "message-queued",
          message_id: "m",
          intent: "inform",
          text: "Check this",
        },
        "dev.b@r",
      ),
      actor: { kind: "seat" as const, id: "dev.a@r" },
    };
    const result = liveProjection(c, seats, [queued]);
    expect(result.compiled.edges).toHaveLength(1);
    expect(result.labels["dev.a@r→dev.b@r"]).toBe("1 message observed");
  });
  it("shows only durable managed parent links and removes unknown-status agents", () => {
    const c = compiledRig();
    c.edges.push({
      from: "dev.b@r",
      to: "dev.a@r",
      kind: "spawned_by",
      enforced: true,
    });
    const seats = { "dev.a@r": seatView("a"), "dev.b@r": seatView("b") };
    expect(liveProjection(c, seats, []).compiled.edges).toHaveLength(0);
    expect(
      liveProjection(c, seats, [], undefined, [
        { parent: "dev.a@r", child: "dev.b@r" },
      ]).compiled.edges,
    ).toHaveLength(1);
    seats["dev.b@r"].lifecycle = "stale";
    const result = liveProjection(c, seats, [], undefined, [
      { parent: "dev.a@r", child: "dev.b@r" },
    ]);
    expect(result.compiled.seats).toHaveLength(1);
    expect(result.unknown).toBe(1);
  });
});
