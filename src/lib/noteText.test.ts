import { EditorSelection, EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";

import { withoutImage } from "./cardImages";
import { changeBetween, noteLimit } from "./noteText";

/** No half of a character: no surrogate without its other half. */
const whole = (text: string): boolean => !/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(text);

/** What the card dialog's editor holds after it is told the note is now `to`. */
function told(from: string, to: string): string {
  const state = EditorState.create({ doc: from });
  const change = changeBetween(from, to);
  return change ? state.update({ changes: change }).state.doc.toString() : from;
}

/** The note after `insert` is typed or pasted over `from`..`to` of `doc`, kept within `max`. */
function typed(doc: string, insert: string, from: number, to = from, max = 20): string {
  const state = EditorState.create({ doc, extensions: noteLimit(max) });
  return state.update({ changes: { from, to, insert }, userEvent: "input.paste" }).state.doc.toString();
}

describe("a card's note in its editor", () => {
  const note = "Fix the 😀 rounding\n\n- [ ] totals [Image #1] here\n- [x] tax [Image #2]";

  it.each([
    ["an image taken out by its button", note, withoutImage(note, 1)],
    ["the last image taken out", note, withoutImage(note, 2)],
    ["an emoji changed for another", "a😀b", "a😁b"],
    ["an emoji put before its twin", "😀", "😀😀"],
    ["a note emptied", note, ""],
    ["an empty note written", "", note],
    ["the same note", note, note],
  ])("is what it was told after %s, character for character", (_, from, to) => {
    expect(told(from, to)).toBe(to);
    const change = changeBetween(from, to);
    if (change) expect([change.insert, from.slice(0, change.from), from.slice(change.to)].every(whole)).toBe(true);
  });

  it("cuts what is pasted past its length to what fits and keeps the rest of the note", () => {
    expect(typed("0123456789abcdef", "WXYZ!!", 16)).toBe("0123456789abcdefWXYZ");
    expect(typed("0123456789abcdef", "WXYZ!!", 4, 4)).toBe("0123WXYZ456789abcdef");
    expect(typed("0123456789abcdefghij", "12345", 2, 4)).toBe("0112456789abcdefghij");
  });

  it("never keeps half of a character it cuts", () => {
    expect(typed("0123456789abcdefghi", "😀", 19)).toBe("0123456789abcdefghi");
    expect(typed("0123456789abcdefgh", "😀😀", 18)).toBe("0123456789abcdefgh😀");
  });

  it("lets go of an over-long change in several places at once, and always of a deletion", () => {
    const doc = "0123456789abcdefghi";
    const state = EditorState.create({ doc, extensions: [noteLimit(20), EditorState.allowMultipleSelections.of(true)], selection: EditorSelection.create([EditorSelection.cursor(1), EditorSelection.cursor(5)]) });
    expect(state.update(state.replaceSelection("xy")).state.doc.toString()).toBe(doc);
    expect(typed("0123456789abcdefghijKLM", "", 20, 23)).toBe("0123456789abcdefghij");
  });
});
