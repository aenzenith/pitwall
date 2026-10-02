<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { uptime } from "../../lib/format";
import { t } from "../../lib/i18n";
import { useNativeMenu } from "../../lib/nativeMenu";
import { api, now } from "../../lib/store";
import type { CommandView, Project } from "../../lib/types";

/** `tab`: the output tab shown, whose command row is lit. */
const props = defineProps<{ project: Project; tab: string | null }>();
/** A command waiting for "Run?" to be confirmed. */
const confirming = defineModel<string | null>("confirming", { required: true });
/** `show`: its output tab; `edit`: the command dialog (`null` for a new command). */
const emit = defineEmits<{ show: [id: string]; edit: [id: string | null]; remove: [id: string] }>();

const commands = computed(() => props.project.commands ?? []);
const menu = useNativeMenu();

function mark(c: CommandView): string {
  return { running: "●", busy: "", ok: "✓", failed: "✕", idle: "·" }[c.status];
}

function run(c: CommandView): void {
  if (c.confirm && confirming.value !== c.id) {
    confirming.value = c.id;
    return;
  }
  confirming.value = null;
  void api.runCommand(props.project.path, c.id);
  emit("show", c.id);
}

function stop(c: CommandView): void {
  void api.stopCommand(props.project.path, c.id);
}

/** Right-click on a command: the native menu with what the row can do. */
function commandMenu(c: CommandView, event: MouseEvent): void {
  const running = c.status === "running";
  void menu.popup(event, [
    { text: t(running ? "common.stop" : "common.run"), enabled: c.status !== "busy", action: () => (running ? stop(c) : run(c)) },
    { text: t("detail.menuEdit"), action: () => emit("edit", c.id) },
    "separator",
    { text: t("common.delete"), action: () => emit("remove", c.id) },
  ]);
}
</script>

<template>
  <section class="block commands" :aria-label="t('common.commands')">
    <div class="commands-head">
      <div class="section-label">{{ t("common.commands") }}</div>
      <button type="button" class="text-button" @click="emit('edit', null)">{{ t("detail.add") }}</button>
    </div>
    <p v-if="!commands.length" class="empty-commands">{{ t("detail.noCommands") }}</p>
    <div v-for="c in commands" :key="c.id" :class="['cmd-row', { focused: tab === c.id }]" @contextmenu="commandMenu(c, $event)">
      <template v-if="confirming === c.id">
        <span class="confirm-text">{{ t("detail.confirmRun", { name: c.name }) }}</span>
        <button type="button" class="control small-btn" @click="confirming = null">{{ t("common.cancel") }}</button>
        <button type="button" class="control primary small-btn" @click="run(c)">{{ t("common.run") }}</button>
      </template>
      <template v-else>
        <span :class="['mark', c.status]">
          <Spinner v-if="c.status === 'busy'" :size="10" />
          <template v-else>{{ mark(c) }}</template>
        </span>
        <button type="button" class="cmd-open" :title="t('detail.showOutput', { name: c.name })" @click="emit('show', c.id)">
          <span class="cmd-name">{{ c.name }}</span>
          <span class="cmd-line">{{ c.command }}</span>
        </button>
        <span v-if="c.status === 'running'" class="cmd-state">{{ uptime(c.startedAt, now) }}</span>
        <button type="button" class="icon cmd-edit" :aria-label="t('common.editName', { name: c.name })" :title="t('common.edit')" @click="emit('edit', c.id)">
          <Icon name="pencil" :size="13" />
        </button>
        <button
          v-if="c.status === 'running'"
          type="button"
          class="icon"
          :aria-label="t('common.stopName', { name: c.name })"
          :title="t('common.stop')"
          @click="stop(c)"
        >
          <Icon name="stop" :size="12" />
        </button>
        <button
          v-else
          type="button"
          class="icon"
          :disabled="c.status === 'busy'"
          :aria-label="t('common.runName', { name: c.name })"
          :title="t('common.run')"
          @click="run(c)"
        >
          <Icon name="play" :size="12" />
        </button>
      </template>
    </div>
  </section>
</template>

<style scoped src="./detail.css"></style>
<style scoped>
/* Commands: no line under it; the output brings its own. */
.commands {
  gap: 4px;
  border-bottom: 0;
}

/* The highlight reaches 8px past the block on both sides; the content keeps the block's edges,
   so the run button sits as far from the side as the dev server's buttons. */
.cmd-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 40px;
  margin: 0 -8px;
  padding: 0 8px;
  border-radius: var(--radius-control);
}

.cmd-row.focused {
  background: #1b1e24;
}

.mark {
  width: 16px;
  display: inline-flex;
  justify-content: center;
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-faint);
}

.mark.running {
  color: var(--run);
  font-size: 10px;
}

.mark.ok {
  color: var(--run);
}

.mark.failed {
  color: var(--crash-text);
}

.mark.busy {
  color: var(--text-muted);
}

.cmd-open {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex-grow: 1;
  min-width: 0;
  padding: 4px 0;
  border: 0;
  background: transparent;
  text-align: left;
}

/* The edit button shows on hover, like a list row's accessory; it keeps its place either way. */
.cmd-edit {
  margin-right: -6px;
  opacity: 0;
}

.cmd-row:hover .cmd-edit,
.cmd-edit:focus-visible {
  opacity: 1;
}

/* Only while it runs: how long it has been up. */
.cmd-state {
  font-size: 12px;
  white-space: nowrap;
  color: #c7ccd3;
}

.confirm-text {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
  color: #f3c29b;
}

.small-btn {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}
</style>
