// A page's stretch of time: today, an earlier day, or every day there is something kept of. The
// Board and Sessions pages each show one (components/PeriodPicker). No component state; what
// reads the language (`t`, `clock`) follows it.

import { clock, dayName, yesterday } from "./day";
import { language, t } from "./i18n";

/** Every day there is something kept of. */
export const ALL = "all";

/** `ALL`, an earlier day (`YYYY-MM-DD`, local), or null for today. */
export type Period = string | null;

/** Noon of a `YYYY-MM-DD` day: a step from there lands on the next day whatever the clocks do. */
function noon(date: string): Date {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day, 12);
}

/** The day `days` after `date` (before it, when negative). */
export function shiftDay(date: string, days: number): string {
  const d = noon(date);
  d.setDate(d.getDate() + days);
  return dayName(d.getTime());
}

/** The day before the one shown; undefined from every day's, where there is none to step from. */
export function previousDay(period: Period): Period | undefined {
  if (period === ALL) return undefined;
  return period === null ? yesterday() : shiftDay(period, -1);
}

/** The day after the one shown (null: today); undefined from today and from every day's. */
export function nextDay(period: Period): Period | undefined {
  if (period === null || period === ALL) return undefined;
  const next = shiftDay(period, 1);
  return next >= dayName(Date.now()) ? null : next;
}

/** `at` falls in `period`. */
export function inPeriod(at: number, period: Period, now: number): boolean {
  return period === ALL || dayName(at) === (period ?? dayName(now));
}

/** One date formatter per language and shape, made on first use: a list has hundreds of days. */
const formats = new Map<string, Intl.DateTimeFormat>();

function format(shape: "short" | "year" | "long"): Intl.DateTimeFormat {
  const key = `${language.value} ${shape}`;
  let found = formats.get(key);
  if (!found) {
    const options: Intl.DateTimeFormatOptions =
      shape === "long" ? { weekday: "long", day: "numeric", month: "long" } : { day: "numeric", month: "short", year: shape === "year" ? "numeric" : undefined };
    found = new Intl.DateTimeFormat(language.value, options);
    formats.set(key, found);
  }
  return found;
}

/** `2 Oct`, whatever its year: for a label that keeps to a width. */
export function dayMonth(date: string): string {
  return format("short").format(noon(date));
}

/** `2 Oct`; with its year when that isn't this one. */
export function dayShort(date: string): string {
  const d = noon(date);
  return d.getFullYear() === new Date().getFullYear() ? dayMonth(date) : format("year").format(d);
}

/** `Friday 2 October`. */
export function dayLong(date: string): string {
  return format("long").format(noon(date));
}

/** A period by name: `Today`, `Yesterday`, `2 Oct`, `All time`. */
export function periodLabel(period: Period): string {
  if (period === null) return t("day.today");
  if (period === ALL) return t("period.all");
  return period === yesterday() ? t("day.yesterday") : dayShort(period);
}

/** A time as a row says it: `14:05` today, `2 Oct 14:05` on any other day. */
export function stamp(at: number, now: number): string {
  const day = dayName(at);
  return day === dayName(now) ? clock(at) : `${dayShort(day)} ${clock(at)}`;
}
