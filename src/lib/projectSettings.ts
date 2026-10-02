import { shallowReactive, watch } from "vue";

import { api, snapshot } from "./store";
import type { Project, ProjectSettings } from "./types";

/**
 * The one writer of a project's own settings (`set_project_settings` replaces them whole).
 *
 * A snapshot only shows a change on the next `state` event, so building each change from it
 * would lose the one before (two quick clicks on suggestions, a link then the server fields).
 * Here every change lands on the newest copy, written or not yet seen, and goes out in order;
 * the copy is dropped once a snapshot shows it, or once the core has answered and the snapshot
 * has had time to catch up.
 */

/** Per project path: settings sent but not yet seen in a snapshot. */
const pending = shallowReactive(new Map<string, ProjectSettings>());
/** Per project path: writes not answered yet, and when the last one was. */
const writes = new Map<string, { inFlight: number; answeredAt: number }>();
/** A snapshot that still doesn't show the written settings this long after the answer wins. */
const CATCH_UP_MS = 1500;

let queue: Promise<unknown> = Promise.resolve();

/** Key order and empty fields don't count: the core leaves out empty lists and unset values. */
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(value).sort()) {
      const field = (value as Record<string, unknown>)[key];
      if (field === undefined || field === null || (Array.isArray(field) && !field.length)) continue;
      out[key] = canonical(field);
    }
    return out;
  }
  return value;
}

function same(a: ProjectSettings, b: ProjectSettings): boolean {
  return JSON.stringify(canonical(a)) === JSON.stringify(canonical(b));
}

function stored(path: string): ProjectSettings {
  const snap = snapshot.value;
  return snap?.projects.find((p) => p.path === path)?.settings ?? snap?.settings.projects[path] ?? {};
}

function clone(settings: ProjectSettings): ProjectSettings {
  return JSON.parse(JSON.stringify(settings)) as ProjectSettings;
}

/** The project's settings as this window last wrote them: the snapshot's, or a newer change on top. */
export function projectSettings(project: Project): ProjectSettings {
  return pending.get(project.path) ?? project.settings;
}

/** Applies `change` to the newest settings and sends the result after any earlier write. */
export function updateProjectSettings(path: string, change: (settings: ProjectSettings) => ProjectSettings): Promise<void> {
  const next = change(clone(pending.get(path) ?? stored(path)));
  pending.set(path, next);

  const state = writes.get(path) ?? { inFlight: 0, answeredAt: 0 };
  state.inFlight += 1;
  writes.set(path, state);

  const write = queue.then(() => api.setProjectSettings(path, next));
  queue = write.catch(() => undefined);

  return write
    .then(
      () => undefined,
      () => {
        // Not written: what the core has is the truth again.
        if (state.inFlight === 1) pending.delete(path);
      },
    )
    .finally(() => {
      state.inFlight -= 1;
      state.answeredAt = Date.now();
      settle(path);
    });
}

/** The detail moved to another project: the copy of the one it leaves goes once nothing is on its way. */
export function forgetProjectSettings(path: string): void {
  if (!writes.get(path)?.inFlight) {
    pending.delete(path);
    writes.delete(path);
  }
}

function settle(path: string): void {
  const copy = pending.get(path);
  if (!copy) return;

  const state = writes.get(path);
  const answered = !state?.inFlight;
  if (answered && same(copy, stored(path))) {
    pending.delete(path);
    writes.delete(path);
  } else if (answered && state && Date.now() - state.answeredAt > CATCH_UP_MS) {
    // The snapshot is newer than the write and still differs: someone else changed them.
    pending.delete(path);
    writes.delete(path);
  }
}

watch(snapshot, () => {
  for (const path of [...pending.keys()]) settle(path);
});
