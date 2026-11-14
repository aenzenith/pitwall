<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import { SECTIONS, type MenuCommand } from "../../lib/chat/commands";
import { t } from "../../lib/i18n";

/**
 * The composer's `/` menu (C3, V53–V55): the commands the CLI lists, sectioned (built-in, skills
 * and commands, plugins', MCP prompts), each with its argument hint and description; the window's
 * own dialogs and the terminal's flows among the built-ins, marked. The field keeps the keyboard:
 * the composer moves `active` and picks; a click picks too.
 */
const props = defineProps<{
  items: MenuCommand[];
  active: number;
  /** Nothing typed after `/`: the sections' heads show. */
  sectioned: boolean;
  place: Record<string, string>;
  /** The menu's id, for the field's `aria-controls`; each row is `${id}-${index}`. */
  id: string;
}>();
const emit = defineEmits<{ pick: [index: number]; hover: [index: number] }>();

const list = ref<HTMLElement | null>(null);

type Row = { kind: "head"; key: string; label: string } | { kind: "item"; key: string; index: number; command: MenuCommand };

const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  let section: string | null = null;
  props.items.forEach((command, index) => {
    if (props.sectioned && command.section !== section) {
      section = command.section;
      const head = SECTIONS.find((each) => each.id === section);
      if (head) out.push({ kind: "head", key: `h-${section}`, label: t(head.key) });
    }
    out.push({ kind: "item", key: command.name, index, command });
  });
  return out;
});

// The row the keyboard is on stays in view.
watch(
  () => props.active,
  () =>
    void nextTick(() => {
      const row = list.value?.querySelector<HTMLElement>(".row.hv");
      row?.scrollIntoView({ block: "nearest" });
    }),
);
</script>

<template>
  <Teleport to="body">
    <div :id="id" ref="list" class="pop" role="listbox" :aria-label="t('chatInput.menu.commands')" :style="place" @mousedown.prevent>
      <div v-if="!items.length" class="empty">{{ t("chatInput.menu.noCommand") }}</div>
      <template v-for="row in rows" :key="row.key">
        <div v-if="row.kind === 'head'" class="mh" aria-hidden="true">{{ row.label }}</div>
        <div
          v-else
          :id="`${id}-${row.index}`"
          role="option"
          :aria-selected="active === row.index"
          :class="['row', { hv: active === row.index }]"
          @mouseenter="emit('hover', row.index)"
          @click="emit('pick', row.index)"
        >
          <span class="nm">/{{ row.command.name }}</span>
          <span v-if="row.command.hint" class="hint">{{ row.command.hint }}</span>
          <span class="desc">{{ row.command.description }}</span>
          <span v-if="row.command.via === 'terminal'" class="via" :title="t('chatInput.menu.inTerminal')"><Icon name="terminal" :size="13" /></span>
          <span v-else-if="row.command.via === 'dialog' || row.command.via === 'settings'" class="via" :title="t('chatInput.menu.inWindow')"><Icon name="window" :size="13" /></span>
        </div>
      </template>
    </div>
  </Teleport>
</template>

<style scoped>
.pop {
  position: fixed;
  z-index: 70;
  box-sizing: border-box;
  padding: 5px;
  overflow-y: auto;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
}

.mh {
  padding: 8px 10px 4px;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.07em;
  text-transform: uppercase;
  color: var(--text-faint);
}

:lang(zh) .mh,
:lang(ja) .mh {
  letter-spacing: 0;
}

.row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  height: 28px;
  line-height: 28px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: default;
}

.row.hv {
  background: var(--bg-lit);
}

.nm {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12.5px;
  color: var(--text-strong);
}

.hint {
  flex-shrink: 1;
  max-width: 34%;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-faint);
}

.desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-subtle);
}

.via {
  flex-shrink: 0;
  align-self: center;
  display: inline-flex;
  color: var(--text-faint);
}

.empty {
  padding: 8px 10px;
  color: var(--text-faint);
}
</style>
