import { t, type Key } from "../../lib/i18n";

/** `3 commands, 2 links`: each count that isn't zero with its word, in the app's language. */
export function counted(parts: Array<[Key, number]>): string {
  return parts
    .filter(([, count]) => count > 0)
    .map(([key, count]) => t(key, { count }))
    .join(", ");
}
