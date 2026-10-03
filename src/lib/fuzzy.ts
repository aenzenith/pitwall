// Fuzzy search over projects, as the quick switcher does it; the window's project search uses the
// same match and order.

import type { Project } from "./types";

/** A match: higher scores are better; `hits` are the matched characters' places in the text. */
export type FuzzyMatch = { score: number; hits: number[] };

/** A project that matches, with the matched characters of its name. */
export type ProjectMatch = { project: Project; score: number; hits: Set<number> };

/** Accents written as their own characters (U+0300 to U+036F). */
const MARKS = /[̀-ͯ]/g;

/**
 * `text` as the search compares it: lower case, accents dropped, Turkish's dotless ı as i. Every
 * character keeps its place, so a match's hits fit the text as written: "Bağımlılıklar" →
 * "bagimliliklar".
 */
export function fold(text: string): string {
  let out = "";

  for (const ch of text) {
    const [base, ...marks] = ch.normalize("NFD");
    const plain = marks.length && marks.every((mark) => mark >= "̀" && mark <= "ͯ") ? base : ch;
    const lower = plain === "ı" ? "i" : plain.toLowerCase();
    out += lower.length === ch.length ? lower : ch;
  }

  return out;
}

/**
 * Fuzzy: every query character in order; runs, word starts and prefixes score higher. Case and
 * accents don't count, in the text or in `q` (`fold`): "bagim" finds "Bağımlılıklar".
 */
export function fuzzy(text: string, q: string): FuzzyMatch | null {
  const lower = fold(text);
  // A query has no places to keep: an accent left on its own (an "İ" lowered by the caller) goes.
  const wanted = fold(q.normalize("NFC")).replace(MARKS, "");
  const hits: number[] = [];
  let from = 0;
  let last = -2;
  let score = 0;

  for (const ch of wanted) {
    const i = lower.indexOf(ch, from);
    if (i < 0) return null;
    score += i === last + 1 ? 3 : 1;
    if (i === 0 || /[-_ ./]/.test(lower[i - 1])) score += 2;
    hits.push(i);
    last = i;
    from = i + 1;
  }

  if (lower.startsWith(wanted)) score += 8;
  return { score: score - lower.length * 0.01, hits };
}

/** How much a project wants you, lowest first: Claude waits, Claude works, its server runs, the rest. */
export function rank(p: Project): number {
  if (p.claude) return 0;
  if (p.claudeWorking) return 1;
  if (p.status === "running" || p.status === "busy") return 2;
  return 3;
}

/** Every project, the ones that want you first, then by name: the switcher with nothing typed. */
export function byRank(projects: Project[]): Project[] {
  return [...projects].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}

/**
 * The projects that match `q` (trimmed, lower case), best first: by name, else by branch or path,
 * which count for less; among near equals, the ones that want you first. Ties keep the list's order.
 */
export function searchProjects(projects: Project[], q: string): ProjectMatch[] {
  const found: ProjectMatch[] = [];

  for (const project of projects) {
    const byName = fuzzy(project.name, q);
    const byBranch = project.git ? fuzzy(project.git.branch, q) : null;
    const byPath = fuzzy(project.path, q);
    const best = Math.max(byName?.score ?? -Infinity, (byBranch?.score ?? -Infinity) * 0.6, (byPath?.score ?? -Infinity) * 0.4);

    if (best > -Infinity) {
      found.push({ project, score: best - rank(project) * 0.5, hits: new Set(byName?.hits ?? []) });
    }
  }

  return found.sort((a, b) => b.score - a.score);
}

/** `text` character by character, each marked if the match hit it. */
export function segments(text: string, hits: Set<number>): Array<{ text: string; hit: boolean }> {
  return [...text].map((ch, i) => ({ text: ch, hit: hits.has(i) }));
}
