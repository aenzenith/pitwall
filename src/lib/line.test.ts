import { describe, expect, it } from "vitest";

import { linePlaces, placeAtRefill } from "./line";
import type { Card, CardSchedule } from "./types";

// The core gives the refill's cards in the list's order of projects, each one's Up next from the
// top; the board's file keeps every project's cards mixed. A place counted in the file's order
// tells the user a card goes before another that the core starts first.
describe("the refill's line", () => {
  const refill = 1_800_000_000_000;
  const card = (id: string, path: string, schedule: CardSchedule | null, column: Card["column"] = "queued"): Card => ({
    id,
    path,
    title: id,
    note: "",
    column,
    createdAt: 0,
    movedAt: 0,
    session: null,
    terminal: null,
    givenAt: null,
    images: [],
    schedule,
  });
  const at = (moment: number): CardSchedule => ({ at: moment, refill: true, start: {} });
  const lined: CardSchedule = { at: refill - 1, refill: true, start: {}, line: true };

  // In the file's order; the list says /a first.
  const board = [
    card("b1", "/b", at(refill)),
    card("a1", "/a", at(refill)),
    card("b2", "/b", at(refill + 400)),
    card("a2", "/a", null),
    card("a3", "/a", at(refill)),
    card("gone", "/unlisted", at(refill)),
    card("next", "/a", at(refill + 5 * 60 * 60 * 1000)),
    card("bl", "/b", lined),
    card("al", "/a", lined),
    card("done", "/a", at(refill), "review"),
  ];
  const paths = ["/a", "/b"];

  it("counts each line in the list's order, a project's cards from the top", () => {
    expect(Object.fromEntries(linePlaces(board, paths))).toEqual({ a1: 1, a3: 2, b1: 3, b2: 4, next: 1, al: 1, bl: 2 });
  });

  it("puts a card told to wait now where its project and place take it", () => {
    expect(placeAtRefill(board, paths, "a2", refill)).toBe(2);
    expect(placeAtRefill([...board, card("b3", "/b", null)], paths, "b3", refill)).toBe(5);
    expect(placeAtRefill([...board, card("u2", "/unlisted", null)], paths, "u2", refill)).toBeNull();
  });
});
