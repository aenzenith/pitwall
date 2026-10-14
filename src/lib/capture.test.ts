import { describe, expect, it } from "vitest";

import { useAdd, type AddPort, type Draft } from "./capture";
import type { Card, NewImage } from "./types";

/** A board that keeps what it is asked to add, and answers when told to. */
function board() {
  const asked: Array<{ path: string; title: string; note: string; images: NewImage[] }> = [];
  const answers: Array<{ resolve: (card: Card) => void; reject: (why: Error) => void }> = [];
  const add: AddPort["add"] = (path, title, note, images) => {
    asked.push({ path, title, note, images });
    return new Promise<Card>((resolve, reject) => answers.push({ resolve, reject }));
  };
  const card = (n: number): Card => ({ id: `c${n}`, path: asked[n].path, title: asked[n].title, note: asked[n].note, column: "queued", createdAt: 0, movedAt: 0, session: null, terminal: null, givenAt: null, images: [] });
  return { asked, add, answer: (n: number) => answers[n].resolve(card(n)), refuse: (n: number) => answers[n].reject(new Error("refused")) };
}

function port(add: AddPort["add"]): AddPort {
  return { target: () => "/projects/shop", images: { settled: () => Promise.resolve(), taken: () => [], clear: () => undefined }, refused: () => false, add };
}

describe("a card written in the quick switcher", () => {
  it("is added once however often Return comes before the board answers", async () => {
    const written: Draft = { title: "Rounding is off", note: "", path: "/projects/shop" };
    const there = board();
    const { add } = useAdd(written, port(there.add));

    const first = add();
    const second = add();
    const third = add();
    await Promise.resolve();
    await Promise.resolve();
    expect(there.asked).toHaveLength(1);

    there.answer(0);
    expect((await first)?.id).toBe("c0");
    expect(await second).toBeNull();
    expect(await third).toBeNull();
    expect(there.asked).toHaveLength(1);
  });

  it("leaves nothing for the next Return once it is on the board", async () => {
    const written: Draft = { title: "Rounding is off", note: "Twice per line.", path: "/projects/shop" };
    const there = board();
    const { add } = useAdd(written, port(there.add));

    const first = add();
    await Promise.resolve();
    await Promise.resolve();
    there.answer(0);
    await first;

    expect(written).toEqual({ title: "", note: "", path: null });
    expect(await add()).toBeNull();
    expect(there.asked).toHaveLength(1);
  });

  it("stays written when the board refuses it, and is added once when tried again", async () => {
    const written: Draft = { title: "Rounding is off", note: "Twice per line.", path: "/projects/shop" };
    const there = board();
    const { add, adding } = useAdd(written, port(there.add));

    const first = add();
    await Promise.resolve();
    await Promise.resolve();
    there.refuse(0);
    await expect(first).rejects.toThrow("refused");
    expect(written).toEqual({ title: "Rounding is off", note: "Twice per line.", path: "/projects/shop" });
    expect(adding.value).toBe(false);

    const again = add();
    await Promise.resolve();
    await Promise.resolve();
    there.answer(1);
    expect((await again)?.id).toBe("c1");
    expect(there.asked).toHaveLength(2);
  });
});
