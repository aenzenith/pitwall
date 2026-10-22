<script setup lang="ts">
import { computed, nextTick, ref } from "vue";

import Icon from "../components/Icon.vue";
import { parseKey, rowKey, useGroupReorder } from "../lib/groupReorder";
import { t } from "../lib/i18n";
import { useNativeMenu } from "../lib/nativeMenu";
import { dragRegion, keys } from "../lib/platform";
import { snapshot } from "../lib/store";
import { launchTeam, membersOf, setTeamProjects, startTeam, stopTeam, teamAsk, teamOf, teamState } from "../lib/teams";
import type { Team } from "../lib/types";
import SwapLabel from "./team/SwapLabel.vue";
import TeamClaude from "./team/TeamClaude.vue";

/**
 * A team's details, in the pane a project's take: its name, starting and stopping it, the order
 * its servers start in, whether they start with Pitwall, and deleting it. Renaming and deleting
 * ask in the window's own dialogs (lib/teams: teamAsk).
 */
const props = defineProps<{ team: Team }>();

const members = computed(() => membersOf(props.team));
const state = computed(() => teamState(props.team));

/* ---------- start and stop ---------- */

/** "Start the rest" while some of them run already; both labels lie in the button, so it keeps one width. */
const startLabels = computed(() => [
  { id: "all", text: t("team.start"), icon: "play" as const },
  { id: "rest", text: t("team.startRest"), icon: "play" as const },
]);
const startShown = computed(() => (state.value.starting ? null : state.value.running > 0 && state.value.running < state.value.total ? "rest" : "all"));
const stopLabels = computed(() => [{ id: "stop", text: t("team.stop"), icon: "stop" as const }]);

/* ---------- the start order ---------- */

const list = ref<HTMLElement | null>(null);
const drag = useGroupReorder(list, (change) => void setTeamProjects(props.team.id, change.keys.map((key) => parseKey(key).id)));
const menu = useNativeMenu();

/** The listed projects that aren't in this team: what "Add project" offers. */
const others = computed(() => (snapshot.value?.projects ?? []).filter((project) => !props.team.projects.includes(project.path)));

function paths(): string[] {
  return members.value.map((project) => project.path);
}

/** The native menu under the button: a project in another team says which, as it would leave it. */
function addMenu(event: MouseEvent): void {
  const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
  void menu.popup(
    { clientX: box.left, clientY: box.bottom + 4 },
    others.value.map((project) => {
      const other = teamOf(project.path);
      return { text: other ? `${project.name} · ${other.name}` : project.name, action: () => void setTeamProjects(props.team.id, [...paths(), project.path]) };
    }),
  );
}

/** The project leaves the team; it stays listed. */
function remove(path: string): void {
  void setTeamProjects(
    props.team.id,
    paths().filter((member) => member !== path),
  );
}

/** ⌥↑ and ⌥↓ on a project move it in the order; the keyboard stays on it. */
function onMemberKey(event: KeyboardEvent, at: number): void {
  if (!event.altKey || event.metaKey || event.ctrlKey || event.target !== event.currentTarget) return;
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
  event.preventDefault();
  const to = event.key === "ArrowUp" ? at - 1 : at + 1;
  const order = paths();
  if (to < 0 || to >= order.length) return;
  const path = order[at];
  [order[at], order[to]] = [order[to], order[at]];
  void setTeamProjects(props.team.id, order);
  void nextTick(() => Array.from(list.value?.querySelectorAll<HTMLElement>("[data-path]") ?? []).find((row) => row.dataset.path === path)?.focus());
}
</script>

<template>
  <section class="detail" :aria-label="t('team.details')">
    <!-- The name and the line under it drag the window; the rename button stays clickable. -->
    <header class="head" :data-tauri-drag-region="dragRegion">
      <div class="title-row">
        <div class="name">{{ team.name }}</div>
        <button type="button" class="icon" :aria-label="t('team.rename')" :title="t('team.rename')" @click="teamAsk = { kind: 'rename', team }">
          <Icon name="pencil" :size="16" />
        </button>
      </div>
      <div class="sub">
        <span class="sub-line">{{ t("team.kind") }} · {{ t("team.projects", { count: state.total }) }}<template v-if="state.running"> · <span class="live">{{ t("team.running", { count: state.running }) }}</span></template><span v-if="state.claude.length" class="sep" aria-hidden="true">·</span></span>
        <TeamClaude :notes="state.claude" />
      </div>
    </header>

    <div class="sections">
      <!-- Each spins until the servers are really up or down; a start can be stopped on its way. -->
      <section class="block" :aria-label="t('common.devServer')">
        <div class="run">
          <button
            type="button"
            class="control start"
            :disabled="state.busy || !state.total || state.running === state.total"
            :aria-busy="state.starting"
            :aria-label="state.starting ? t('team.start') : undefined"
            @click="startTeam(team.id)"
          >
            <SwapLabel :labels="startLabels" :show="startShown" />
          </button>
          <button
            type="button"
            class="control stop"
            :disabled="state.stopping || (!state.running && !state.starting)"
            :aria-busy="state.stopping"
            :aria-label="state.stopping ? t('team.stop') : undefined"
            @click="stopTeam(team.id)"
          >
            <SwapLabel :labels="stopLabels" :show="state.stopping ? null : 'stop'" />
          </button>
        </div>
      </section>

      <section class="block" :aria-label="t('team.order')">
        <div class="commands-head">
          <div class="section-label">{{ t("team.order") }}</div>
          <button type="button" class="text-button" :disabled="!others.length" @click="addMenu">{{ t("team.addProject") }}</button>
        </div>
        <p v-if="!members.length" class="empty-commands">{{ t("team.noMembers") }}</p>
        <template v-else>
          <span id="team-order-keys" class="sr-only">{{ t("team.orderKeys", { up: keys("Alt+ArrowUp"), down: keys("Alt+ArrowDown") }) }}</span>
          <!-- Drag a project, or move it with the keyboard; its button takes it out of the team. -->
          <ol ref="list" :class="['order', drag.listClass()]" aria-describedby="team-order-keys">
            <li
              v-for="(project, i) in members"
              :key="project.path"
              tabindex="0"
              :data-path="project.path"
              :class="['member', drag.lineClass(rowKey(project.path))]"
              :style="drag.lineStyle(rowKey(project.path))"
              @pointerdown="drag.down($event, rowKey(project.path))"
              @keydown="onMemberKey($event, i)"
            >
              <Icon name="grip" :size="14" class="member-grip" />
              <span class="member-index">{{ i + 1 }}</span>
              <span class="member-name">{{ project.name }}</span>
              <!-- Shown under the pointer or the keyboard. -->
              <button type="button" class="icon small member-remove" data-no-drag :aria-label="t('team.removeName', { name: project.name })" :title="t('team.leave')" @click="remove(project.path)">
                <Icon name="close" :size="13" />
              </button>
            </li>
          </ol>
          <p class="note">{{ t("team.orderHint") }}</p>
        </template>
      </section>

      <section class="block">
        <label class="switch-row">
          <span class="switch-name">{{ t("team.launch") }}</span>
          <input type="checkbox" role="switch" :checked="team.startAtLaunch" @change="launchTeam(team.id, ($event.target as HTMLInputElement).checked)" />
        </label>
      </section>

      <section class="block">
        <button type="button" class="text-button danger" @click="teamAsk = { kind: 'delete', team }">{{ t("team.delete") }}</button>
      </section>
    </div>
  </section>
</template>

<style scoped src="./detail/detail.css"></style>
<style scoped>
.detail {
  width: 380px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-detail);
  border-left: 1px solid var(--line);
  min-height: 0;
  overflow: hidden;
}

/* 94px, as a project's (detail/DetailHeader): toolbar (56) + table header (38), so its line meets
   the table header's line. */
.head {
  height: 94px;
  flex-shrink: 0;
  padding: 13px 16px 0 20px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-bottom: 1px solid var(--line);
}

.title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.name {
  font-size: 17px;
  font-weight: 600;
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The count gives way before what Claude is at does. */
.sub {
  display: flex;
  align-items: center;
  font-size: 12px;
  color: var(--text-subtle);
}

.sub-line {
  flex: 0 1000 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.sub .live {
  color: var(--run);
}

/* The dot before what Claude is at (team/TeamClaude), the count's own: gone with it. */
.sub-line .sep {
  margin-left: 0.25em;
}

.sections {
  flex: 1 1 auto;
  min-height: 0;
  padding-bottom: 24px;
  overflow-y: auto;
  /* Room under the end for the terminal panel lying over the page (WindowApp: `--panel-over`). */
  padding-bottom: calc(24px + var(--panel-over, 0px));
  scroll-padding-bottom: var(--panel-over, 0px);
}
/* Its scrollbar ends where the panel begins: the list keeps its size, only the bar's track is
   shorter (WindowApp: `--panel-over`). */
.sections::-webkit-scrollbar-track {
  margin-bottom: var(--panel-over, 0px);
}


.block:last-child {
  border-bottom: 0;
}

/* Side by side; in a language whose labels are too long for that, one under the other. */
.run {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.run .control {
  flex-shrink: 0;
}

.text-button:disabled {
  opacity: 0.4;
}

/* The start order: one line a project, close like a list. */
.order {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
}

.member {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 36px;
  margin: 0 -8px;
  padding: 0 4px 0 8px;
  border-radius: var(--radius-control);
}

.member:hover {
  background: var(--bg-row-hover);
}

.member:focus-visible {
  outline-offset: -2px;
}

.member-grip {
  flex-shrink: 0;
  color: var(--text-subtle);
}

.member-index {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
  font-variant-numeric: tabular-nums;
}

.member-name {
  flex-grow: 1;
  min-width: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.member-remove {
  flex-shrink: 0;
  opacity: 0;
}

.member:hover .member-remove,
.member:focus-within .member-remove {
  opacity: 1;
}

/* Drag to reorder, as the project list does it (lib/groupReorder). */
.order.reordering .member {
  transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.order.reordering .member:hover:not(.dragging) {
  background: transparent;
}

.order.reordering .member:not(.dragging) .member-remove {
  opacity: 0;
}

.member.dragging {
  position: relative;
  z-index: 2;
  background: var(--bg-selected);
  box-shadow:
    0 14px 22px -14px var(--shade-drag),
    inset 0 0 0 1px var(--line-selected);
}

.order.reordering .member.dragging {
  transition: none;
}

.order.reordering .member.dragging.settling {
  transition: transform 160ms ease-out;
}

.order.frozen .member {
  transition: none !important;
}

@media (prefers-reduced-motion: reduce) {
  .order.reordering .member,
  .order.reordering .member.dragging.settling {
    transition: none;
  }
}

.note {
  margin: 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-subtle);
}

/* A setting and its switch, on one line; a long name gives way. */
.switch-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.switch-name {
  flex-grow: 1;
  min-width: 0;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Deleting is quiet until asked for: its text alone says what it is, its edge on the block's. */
.text-button.danger {
  align-self: flex-start;
  margin-left: -8px;
  color: var(--crash-text);
}

.text-button.danger:hover {
  color: var(--crash-text);
}
</style>
