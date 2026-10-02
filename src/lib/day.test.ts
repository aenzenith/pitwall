import { describe, expect, it } from "vitest";

import { covered } from "./day";

// The day's "Active" figure and every lane's total: overlapping work, waits and server time
// must count once, or the hours shown are quietly too many.
describe("covered", () => {
  const span = (start: number, end: number) => ({ start, end });

  it.each([
    ["nothing", [], 0],
    ["one span", [span(10, 20)], 10],
    ["apart", [span(0, 10), span(20, 25)], 15],
    ["overlapping, counted once", [span(0, 10), span(5, 15)], 15],
    ["one inside another", [span(0, 30), span(5, 10)], 30],
    ["touching", [span(0, 10), span(10, 20)], 20],
    ["out of order", [span(20, 30), span(0, 10), span(5, 25)], 30],
  ])("%s", (_, spans, total) => {
    expect(covered(spans)).toBe(total);
  });
});
