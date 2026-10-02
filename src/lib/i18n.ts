import { computed } from "vue";

import de from "../locales/de.json";
import en from "../locales/en.json";
import es from "../locales/es.json";
import ja from "../locales/ja.json";
import ru from "../locales/ru.json";
import tr from "../locales/tr.json";
import zhHans from "../locales/zh-Hans.json";
import { snapshot } from "./store";

export type Key = keyof typeof en;

/** A text that changes with `count`: the forms `Intl.PluralRules` picks from. */
type Plural = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };

/** Every key English has, so a catalog missing one fails the type check. */
type Catalog = { [K in Key]: (typeof en)[K] extends string ? string : Plural };

/** Every language Pitwall speaks, each under its own name, in the order Settings lists them. */
export const LANGUAGES = [
  { code: "en", name: "English" },
  { code: "tr", name: "Türkçe" },
  { code: "de", name: "Deutsch" },
  { code: "es", name: "Español" },
  { code: "ru", name: "Русский" },
  { code: "ja", name: "日本語" },
  { code: "zh-Hans", name: "简体中文" },
] as const;

export type Language = (typeof LANGUAGES)[number]["code"];

const catalogs: Record<Language, Catalog> = { en, tr, de, es, ru, ja, "zh-Hans": zhHans };

/** What the core resolved (Settings' choice, else the system's); English until it answers. */
export const language = computed<Language>(() => snapshot.value?.language ?? "en");

const rules = new Map<Language, Intl.PluralRules>();

function pluralForm(count: number): Intl.LDMLPluralRule {
  let rule = rules.get(language.value);
  if (!rule) {
    rule = new Intl.PluralRules(language.value);
    rules.set(language.value, rule);
  }
  return rule.select(count);
}

/** The text for `key`; `{name}` placeholders are filled from `params`, and `count` picks the plural form. */
export function t(key: Key, params: Record<string, string | number> = {}): string {
  const entry = catalogs[language.value][key] ?? catalogs.en[key];
  const text = typeof entry === "string" ? entry : (entry[pluralForm(Number(params.count ?? 0))] ?? entry.other);
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

export function languageName(code: Language): string {
  return LANGUAGES.find((l) => l.code === code)?.name ?? code;
}
