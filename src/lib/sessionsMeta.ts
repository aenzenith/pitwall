// The Sessions page's own marks on sessions (O10): archived, unread, and the user's groups. Kept by
// the core as session ids and the groups' names only (`sessions-meta.json` beside the settings);
// read once the page or the terminals' history needs them, changed by one patch at a time
// (`set_sessions_meta`), whose answer is the whole new state.

import { computed, ref } from "vue";

import type { SessionsMeta, SessionsMetaPatch } from "./chat/wire";
import { api } from "./store";

const EMPTY: SessionsMeta = { archived: [], groups: [], unread: [] };

/** As the core last told it; null until it has. */
export const sessionsMeta = ref<SessionsMeta | null>(null);

/** Read leniently: a field missing or of the wrong kind is empty. */
function tidy(meta: unknown): SessionsMeta {
  const raw = (meta ?? {}) as Partial<Record<keyof SessionsMeta, unknown>>;
  const ids = (value: unknown): string[] => (Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : []);
  const groups = Array.isArray(raw.groups)
    ? raw.groups.flatMap((group) => {
        const g = group as { id?: unknown; name?: unknown; sessions?: unknown };
        return typeof g?.id === "string" && typeof g.name === "string" ? [{ id: g.id, name: g.name, sessions: ids(g.sessions) }] : [];
      })
    : [];
  return { archived: ids(raw.archived), groups, unread: ids(raw.unread) };
}

let loading: Promise<SessionsMeta> | null = null;

/** Asked of the core once; again after a failure. */
export function loadSessionsMeta(): Promise<SessionsMeta> {
  if (sessionsMeta.value) return Promise.resolve(sessionsMeta.value);
  loading ??= api.sessionsMeta().then(
    (meta) => (sessionsMeta.value = tidy(meta)),
    () => {
      loading = null;
      return sessionsMeta.value ?? EMPTY;
    },
  );
  return loading;
}

/** One change; false when the core refused it (what is shown stays as stored). */
export async function patchSessionsMeta(patch: SessionsMetaPatch): Promise<boolean> {
  try {
    sessionsMeta.value = tidy(await api.setSessionsMeta(patch));
    return true;
  } catch {
    return false;
  }
}

export const archivedIds = computed(() => new Set(sessionsMeta.value?.archived ?? []));
export const unreadIds = computed(() => new Set(sessionsMeta.value?.unread ?? []));
export const sessionGroups = computed(() => sessionsMeta.value?.groups ?? []);

/** The group a session is in (the first that holds it), if any. */
export function groupOf(id: string): { id: string; name: string } | null {
  const group = sessionGroups.value.find((each) => each.sessions.includes(id));
  return group ? { id: group.id, name: group.name } : null;
}

/** A session was opened (brought up, reopened, opened in a chat): it is no longer unread. */
export async function markOpened(id: string): Promise<void> {
  const meta = sessionsMeta.value ?? (await loadSessionsMeta());
  if (meta.unread.includes(id)) await patchSessionsMeta({ unread: [id], on: false });
}

/** A new group named `name`, and its id (the core makes it); null when it was refused. */
export async function addGroup(name: string): Promise<string | null> {
  const before = new Set(sessionGroups.value.map((group) => group.id));
  if (!(await patchSessionsMeta({ group: { name } }))) return null;
  return sessionGroups.value.find((group) => !before.has(group.id))?.id ?? null;
}
