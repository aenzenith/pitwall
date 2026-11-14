<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { t } from "../../lib/i18n";

/** A row of the `@` menu: a file or a folder of the project, or one of Claude's agents. */
export type MentionItem = { kind: "file" | "dir" | "agent"; key: string; label: string; detail: string };

/**
 * The composer's `@` menu (V38, V39, C6, C8): the project's files and folders that match what was
 * typed (Claude Code's own suggestions, else the Files window's names), and the agents Claude
 * Code listed. The field keeps the keyboard, as in the `/` menu.
 */
const props = defineProps<{ items: MentionItem[]; active: number; loading: boolean; place: Record<string, string>; id: string }>();
const emit = defineEmits<{ pick: [index: number]; hover: [index: number] }>();

const list = ref<HTMLElement | null>(null);

/* The panel's height follows its rows, eased as they narrow or grow (the user's word, 14 Nov): its
   rows are measured as they change, and the panel is given that height, up to the room above the
   box (`place.maxHeight`), past which it scrolls. */
const inner = ref<HTMLElement | null>(null);
const height = ref<number | null>(null);
let sizer: ResizeObserver | null = null;
onMounted(() => {
  sizer = new ResizeObserver(() => {
    if (inner.value) height.value = inner.value.offsetHeight + 12;
  });
  if (inner.value) sizer.observe(inner.value);
});
onBeforeUnmount(() => sizer?.disconnect());

type Row = { kind: "head"; key: string; label: string } | { kind: "item"; key: string; index: number; item: MentionItem };

const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  let group: "agent" | "file" | null = null;
  props.items.forEach((item, index) => {
    const its = item.kind === "agent" ? "agent" : "file";
    if (its !== group) {
      group = its;
      out.push({ kind: "head", key: `h-${its}`, label: t(its === "agent" ? "chatInput.mention.agents" : "chatInput.mention.files") });
    }
    out.push({ kind: "item", key: item.key, index, item });
  });
  return out;
});

watch(
  () => props.active,
  () => void nextTick(() => list.value?.querySelector<HTMLElement>(".row.hv")?.scrollIntoView({ block: "nearest" })),
);

/** A path's file name, and the folder it is in. */
function parts(label: string): { name: string; folder: string } {
  const bare = label.replace(/\/$/, "");
  const cut = bare.lastIndexOf("/");
  return cut < 0 ? { name: label, folder: "" } : { name: label.slice(cut + 1), folder: bare.slice(0, cut) };
}
</script>

<template>
  <Teleport to="body">
    <div :id="id" ref="list" class="pop" role="listbox" :aria-label="t('chatInput.mention.title')" :style="{ ...place, height: height === null ? undefined : `${height}px` }" @mousedown.prevent>
      <div ref="inner">
      <div v-if="!items.length" class="empty">
        <Spinner v-if="loading" :size="12" />
        <span>{{ loading ? t("chatInput.mention.looking") : t("chatInput.mention.none") }}</span>
      </div>
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
          <Icon :name="row.item.kind === 'agent' ? 'users' : row.item.kind === 'dir' ? 'folder' : 'file'" :size="13" class="ic" />
          <template v-if="row.item.kind === 'agent'">
            <span class="nm">{{ row.item.label }}</span>
            <span class="desc">{{ row.item.detail }}</span>
          </template>
          <template v-else>
            <span class="nm mono">{{ parts(row.item.label).name }}</span>
            <span class="desc mono">{{ parts(row.item.label).folder }}</span>
          </template>
        </div>
      </template>
      </div>
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
  transition: height 0.16s ease;
  border: 1px solid var(--line-dialog);
  border-radius: 10px;
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  font-size: 13px;
  color: var(--text);
}

@media (prefers-reduced-motion: reduce) {
  .pop {
    transition: none;
  }
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
  align-items: center;
  gap: 8px;
  height: 28px;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: default;
}

.row.hv {
  background: var(--bg-lit);
}

.ic {
  flex-shrink: 0;
  color: var(--text-muted);
}

.nm {
  flex-shrink: 0;
  max-width: 60%;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-strong);
}

.desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  font-size: 12px;
  color: var(--text-subtle);
}

.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

.desc.mono {
  font-size: 11.5px;
  color: var(--text-faint);
}

.empty {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  color: var(--text-faint);
}
</style>
