// The line of board cards that wait for the 5-hour session's refill. The core gives them one at a
// time (`board_due_at` in src-tauri/src/core/board.rs) in the board's order: the projects as
// listed, then each one's Up next from the top; a card of a folder no longer listed is never
// given. Here: each waiting card's place in its line, where a card told to wait now would stand,
// and what a waiting card says of when it is given.

import { computed } from "vue";

import { t } from "./i18n";
import { timeParams } from "./start";
import { board, snapshot } from "./store";
import type { Card, CardSchedule } from "./types";

/** Two moments this close are one refill: a session's next refill is hours after the last, and the
 * core moves every card that waits for it to each new reading of its moment (`board_refill`). */
const SAME_REFILL_MS = 60_000;

/** Two cards stand in one line: both wait for the same refill, or both had theirs come and wait
 * their turn (`line`), whatever moment their line last moved at. */
function sameLine(a: CardSchedule, b: CardSchedule): boolean {
  if (!!a.line !== !!b.line) return false;
  return !!a.line || Math.abs(a.at - b.at) < SAME_REFILL_MS;
}

/**
 * Each card that waits for the refill, by id: its place in its line, 1 for the one given first.
 * `cards`: the whole board, every project's, in the board's order; `paths`: the listed projects',
 * in the list's order. A card of a folder not among them has no place.
 */
export function linePlaces(cards: Card[], paths: string[]): Map<string, number> {
  const waiting = [...new Set(paths)].flatMap((path) =>
    cards.flatMap((card) => (card.path === path && card.column === "queued" && card.schedule?.refill ? [{ id: card.id, schedule: card.schedule }] : [])),
  );
  const places = new Map<string, number>();
  waiting.forEach(({ id, schedule }, i) => {
    places.set(id, waiting.slice(0, i).filter((ahead) => sameLine(ahead.schedule, schedule)).length + 1);
  });
  return places;
}

/** Where the card `id` would stand, told now to wait for the refill at `at`, among the cards
 * that wait for it already. Null for a card no line takes (not up next, or not of a listed
 * project). */
export function placeAtRefill(cards: Card[], paths: string[], id: string, at: number): number | null {
  const told = cards.map((card) => (card.id === id ? { ...card, schedule: { at, refill: true, start: {} } } : card));
  return linePlaces(told, paths).get(id) ?? null;
}

const listed = computed(() => (snapshot.value?.projects ?? []).map((project) => project.path));

/** `linePlaces` of the board as it is: every card drawn reads its own from it. */
export const places = computed(() => linePlaces(board.value ?? [], listed.value));

/** `placeAtRefill` on the board as it is. */
export function boardPlaceAtRefill(id: string, at: number): number | null {
  return placeAtRefill(board.value ?? [], listed.value, id, at);
}

const englishOrdinal = new Intl.PluralRules("en", { type: "ordinal" });
const ORDINAL_ENDS: Partial<Record<Intl.LDMLPluralRule, string>> = { one: "st", two: "nd", few: "rd" };

/** What a text that names a place takes: the number (`place`), and English's ordinal (`nth`:
 * 1st, 2nd, 3rd, 4th, 11th, 22nd). */
export function placeParams(place: number): { place: number; nth: string } {
  return { place, nth: `${place}${ORDINAL_ENDS[englishOrdinal.select(place)] ?? "th"}` };
}

/** What a waiting card says of when it is given (`place`: its place in its line, null when it has
 * none). Before the refill, the first in line its moment, every other one the moment and its
 * place; once the refill came, that it waits its turn, and where. */
export function waitText(schedule: CardSchedule, place: number | null, lang: string): string {
  if (schedule.line) return place === null ? t("start.waitsTurn") : t("start.waitsTurnAt", placeParams(place));
  const time = timeParams(schedule.at, lang);
  return place !== null && place > 1 ? t("start.givenAfter", { ...time, ...placeParams(place) }) : t("start.givenAt", time);
}
