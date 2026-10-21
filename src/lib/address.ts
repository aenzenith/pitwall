import { ref, watch, type Ref } from "vue";

import { phase, uptime } from "./format";
import { t } from "./i18n";
import { api } from "./store";
import type { Project } from "./types";

/**
 * Where "Open in browser" goes for the project `project` gives, as the core works it out
 * (`resolve_address`): asked again when the project, its server's state or the address it printed
 * changes; `null` while unknown. An older answer, or one for a project no longer shown, is dropped.
 */
export function useAddress(project: () => Project): Ref<string | null> {
  const address = ref<string | null>(null);
  let load = 0;

  watch(
    () => [project().path, project().status, project().url] as const,
    async ([path], before) => {
      // Another project's address is never shown, not even until this one's answer comes.
      if (before && before[0] !== path) address.value = null;
      const mine = ++load;
      const resolved = await api.resolveAddress(path);
      if (mine === load && path === project().path) address.value = resolved;
    },
    { immediate: true },
  );

  return address;
}

/** The server's state, and under it its command and how long it has run (its port too, unless the
 * address line under them shows it already), or what went wrong. */
export function serverLines(project: Project, address: string | null, now: number): { title: string; sub: string } {
  const title = project.status === "busy" ? phase(project) : t(`status.${project.status}`);
  if (project.issue && project.status !== "running") return { title, sub: project.issue.text };
  // The command the dev server runs, as the core runs it (npm, pnpm, yarn or bun).
  const sub = [project.runCommand];
  if (project.status === "running" && project.port && !address?.includes(`:${project.port}`)) sub.push(`:${project.port}`);
  if (project.status === "running" && project.startedAt) sub.push(uptime(project.startedAt, now));
  return { title, sub: sub.filter(Boolean).join(" · ") };
}
