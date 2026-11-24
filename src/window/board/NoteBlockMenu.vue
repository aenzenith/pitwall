<script setup lang="ts">
import Icon from "../../components/Icon.vue";
import { t } from "../../lib/i18n";
import type { NoteBlock } from "../../lib/noteEditor";

/** A block the menu offers: its name, its mark and the Markdown it begins with. */
export type BlockItem = { block: NoteBlock; label: string; icon: "heading" | "list" | "list-ordered" | "checklist" | "editor" | "quote"; markup: string };

/**
 * The note's block menu (CardDialog), opened by a `/` where a block may begin: the blocks what was
 * typed after it matches, the chosen one lit. Drawn inside the dialog (a modal dialog lies over
 * everything else), at `left`/`top` within it. The keyboard stays in the note: ↑/↓, Return and Esc
 * reach the menu through the editor; a click takes a block without taking the keyboard.
 */
defineProps<{ items: BlockItem[]; active: number; left: number; top: number }>();
const emit = defineEmits<{ pick: [block: NoteBlock]; hover: [index: number] }>();
</script>

<template>
  <div class="block-menu" role="listbox" :aria-label="t('board.dialog.blocks')" :style="{ left: `${left}px`, top: `${top}px` }" @mousedown.prevent>
    <div class="bm-head" aria-hidden="true">{{ t("board.dialog.blocks") }}</div>
    <div
      v-for="(item, i) in items"
      :key="item.block"
      role="option"
      :aria-selected="i === active"
      :class="['bm-row', { hv: i === active }]"
      @mouseenter="emit('hover', i)"
      @click="emit('pick', item.block)"
    >
      <span class="bm-mark"><Icon :name="item.icon" :size="13" /></span>
      <span class="bm-label">{{ item.label }}</span>
      <span class="bm-markup">{{ item.markup }}</span>
    </div>
  </div>
</template>

<style scoped>
/* AddMenu's look: Pitwall's own menu, not the system's. */
.block-menu {
  position: absolute;
  z-index: 5;
  width: 248px;
  box-sizing: border-box;
  padding: 5px;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
}

.bm-head {
  height: 24px;
  display: flex;
  align-items: center;
  padding: 0 8px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-faint);
  white-space: nowrap;
}

:lang(zh) .bm-head,
:lang(ja) .bm-head {
  letter-spacing: 0;
}

.bm-row {
  height: 32px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 8px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: pointer;
}

.bm-row.hv {
  background: var(--bg-lit);
}

.bm-mark {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--line-card);
  border-radius: 6px;
  background: var(--bg-control);
  color: var(--text-soft);
}

.bm-label {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bm-markup {
  flex-shrink: 0;
  padding-left: 12px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-faint);
}
</style>
