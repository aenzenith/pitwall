// A card's note as its editor holds it (lib/noteEditor): the text is the note itself, character for
// character, Markdown and `[Image #n]` included; what the editor draws over it changes nothing of
// it. These are the parts that touch the text, apart from the view, so they hold in any setting.

import { EditorSelection, EditorState, Transaction, type Extension } from "@codemirror/state";

/** A note holds this many characters at most: the core's limit, as the field's `maxlength` was. */
export const NOTE_MAX = 8000;

/** The one change that turns `from` into `to`: what lies between their common start and their
 * common end, so an edit made elsewhere (an image taken out by its button) keeps the caret, the
 * undo history and the rest of the note as they were. `null` when they are the same. */
export function changeBetween(from: string, to: string): { from: number; to: number; insert: string } | null {
  if (from === to) return null;
  let start = 0;
  const shorter = Math.min(from.length, to.length);
  while (start < shorter && from.charCodeAt(start) === to.charCodeAt(start)) start++;
  // Never split a surrogate pair: a character outside the BMP is changed whole.
  if (start > 0 && isHighSurrogate(from.charCodeAt(start - 1))) start--;
  let end = 0;
  while (end < shorter - start && from.charCodeAt(from.length - 1 - end) === to.charCodeAt(to.length - 1 - end)) end++;
  if (end > 0 && isLowSurrogate(from.charCodeAt(from.length - end))) end--;
  return { from: start, to: from.length - end, insert: to.slice(start, to.length - end) };
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}

/**
 * Keeps the note within `max` characters, as a text field's `maxlength` does: a typed or pasted
 * insertion that would go past it is cut to what fits (a whole character, never half of one), and
 * one that can't be cut (several places at once) is let go. Deleting is always let through.
 */
export function noteLimit(max = NOTE_MAX): Extension {
  return EditorState.transactionFilter.of((tr) => {
    if (!tr.docChanged || tr.newDoc.length <= max || tr.newDoc.length <= tr.startState.doc.length) return tr;

    const parts: { from: number; to: number; insert: string }[] = [];
    tr.changes.iterChanges((from, to, _fromB, _toB, inserted) => parts.push({ from, to, insert: inserted.toString() }));
    if (parts.length !== 1) return [];

    const [part] = parts;
    const room = max - (tr.startState.doc.length - (part.to - part.from));
    if (room <= 0) return [];
    let insert = part.insert.slice(0, room);
    if (insert.length && isHighSurrogate(insert.charCodeAt(insert.length - 1))) insert = insert.slice(0, -1);
    if (!insert && part.to === part.from) return [];
    return {
      changes: { from: part.from, to: part.to, insert },
      selection: EditorSelection.cursor(part.from + insert.length),
      // What it was (typing, a paste), so the undo history groups it as it would have.
      userEvent: tr.annotation(Transaction.userEvent),
      scrollIntoView: true,
    };
  });
}
