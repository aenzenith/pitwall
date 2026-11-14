<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

import Icon from "../../components/Icon.vue";
import type { TerminalRef } from "../../lib/chat/ui";
import { t } from "../../lib/i18n";

/**
 * The composer's paperclip (V46): a file or folder (the `@` menu), a picture (pasted or dropped),
 * and the last lines of one of the project's terminals (`›` opens them beside it; V41). Its look
 * and keys are the "+" menu's (AddMenu): ↑/↓, → into the terminals, ← and Esc back, Enter picks.
 */
const props = defineProps<{ terminals: TerminalRef[] }>();
const emit = defineEmits<{ mention: []; image: []; terminal: [terminal: TerminalRef] }>();

type Row = { id: "mention" | "image" | "terminal"; label: string; icon: "file" | "eye" | "terminal"; sub: boolean };

const rows = computed<Row[]>(() => [
  { id: "mention", label: t("chatInput.attach.file"), icon: "file", sub: false },
  { id: "image", label: t("chatInput.attach.image"), icon: "eye", sub: false },
  { id: "terminal", label: t("chatInput.attach.terminal"), icon: "terminal", sub: true },
]);

const WIDTH = 230;
const open = ref(false);
const active = ref(0);
const subOpen = ref(false);
const subActive = ref(0);
const inSub = ref(false);
const list = ref<HTMLElement | null>(null);
const subList = ref<HTMLElement | null>(null);
const place = ref<Record<string, string>>({});
const subPlace = ref<Record<string, string>>({});
let anchor: HTMLElement | null = null;
let keyedOpen = false;

/** Opens over `from` (the paperclip), above it as the composer sits at the foot. */
function show(from: HTMLElement, keyed = false): void {
  if (open.value) return hide();
  anchor = from;
  keyedOpen = keyed;
  const box = from.getBoundingClientRect();
  const left = Math.max(8, Math.min(box.left, window.innerWidth - WIDTH - 8));
  const tall = rows.value.length * 30 + 12;
  place.value = box.top - tall - 12 > 0 ? { left: `${left}px`, bottom: `${window.innerHeight - box.top + 6}px`, width: `${WIDTH}px` } : { left: `${left}px`, top: `${box.bottom + 6}px`, width: `${WIDTH}px` };
  open.value = true;
  active.value = 0;
  closeSub();
  window.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", hide);
  window.addEventListener("resize", hide);
  void nextTick(() => list.value?.focus());
}

function hide(): void {
  open.value = false;
  closeSub();
  window.removeEventListener("pointerdown", onOutside, true);
  window.removeEventListener("blur", hide);
  window.removeEventListener("resize", hide);
}

function close(): void {
  hide();
  if (keyedOpen) anchor?.focus();
}

function onOutside(event: PointerEvent): void {
  const target = event.target;
  if (target instanceof Node && (list.value?.contains(target) || subList.value?.contains(target) || anchor?.contains(target))) return;
  hide();
}

function openSub(keyed = false): void {
  const item = list.value?.querySelectorAll<HTMLElement>(".mi")[2]?.getBoundingClientRect();
  const main = list.value?.getBoundingClientRect();
  if (item && main) {
    const width = 230;
    const right = main.right - 4 + width <= window.innerWidth - 8;
    const left = right ? main.right - 4 : Math.max(8, main.left - width + 4);
    subPlace.value = { left: `${left}px`, top: `${item.top - 6}px`, width: `${width}px` };
    void nextTick(() => {
      const tall = subList.value?.offsetHeight ?? 0;
      subPlace.value = { ...subPlace.value, top: `${Math.max(8, Math.min(item.top - 6, window.innerHeight - tall - 8))}px` };
    });
  }
  subOpen.value = true;
  inSub.value = keyed;
  subActive.value = 0;
  if (keyed) void nextTick(() => subList.value?.focus());
}

function closeSub(): void {
  subOpen.value = false;
  inSub.value = false;
}

function pick(at: number): void {
  const row = rows.value[at];
  if (!row) return;
  if (row.sub) return subOpen.value ? closeSub() : openSub();
  close();
  if (row.id === "mention") emit("mention");
  else emit("image");
}

function pickTerminal(at: number): void {
  const terminal = props.terminals[at];
  if (!terminal) return;
  close();
  emit("terminal", terminal);
}

function onKey(event: KeyboardEvent): void {
  if (event.metaKey || event.ctrlKey || event.altKey) return hide();
  const count = inSub.value ? props.terminals.length : rows.value.length;
  switch (event.key) {
    case "ArrowDown":
      if (inSub.value) subActive.value = Math.min(count - 1, subActive.value + 1);
      else active.value = (active.value + 1) % count;
      break;
    case "ArrowUp":
      if (inSub.value) subActive.value = Math.max(0, subActive.value - 1);
      else active.value = (active.value - 1 + count) % count;
      break;
    case "ArrowRight":
      if (!inSub.value && rows.value[active.value]?.sub) openSub(true);
      break;
    case "ArrowLeft":
      if (inSub.value) {
        closeSub();
        void nextTick(() => list.value?.focus());
      }
      break;
    case "Enter":
    case " ":
      if (inSub.value) pickTerminal(subActive.value);
      else if (rows.value[active.value]?.sub) openSub(true);
      else pick(active.value);
      break;
    case "Escape":
      if (inSub.value) {
        closeSub();
        void nextTick(() => list.value?.focus());
      } else close();
      break;
    case "Tab":
      close();
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
}

defineExpose({ show, hide, open });
onBeforeUnmount(hide);
</script>

<template>
  <Teleport to="body">
    <template v-if="open">
      <div ref="list" class="menu" role="menu" tabindex="-1" :aria-label="t('chatInput.attach')" :style="place" @keydown="onKey" @mousedown.prevent>
        <div
          v-for="(row, i) in rows"
          :key="row.id"
          role="menuitem"
          :aria-haspopup="row.sub ? 'menu' : undefined"
          :aria-expanded="row.sub ? subOpen : undefined"
          :class="['mi', { hv: active === i || (row.sub && subOpen) }]"
          @mouseenter="
            active = i;
            row.sub ? openSub() : closeSub();
          "
          @click="pick(i)"
        >
          <Icon :name="row.icon" :size="14" class="ic" />
          <span class="lb">{{ row.label }}</span>
          <Icon v-if="row.sub" name="chevron" :size="13" class="ch" />
        </div>
      </div>
      <div v-if="subOpen" ref="subList" class="menu" role="menu" tabindex="-1" :aria-label="t('chatInput.attach.terminal')" :style="subPlace" @keydown="onKey" @mousedown.prevent>
        <div v-if="!terminals.length" class="none">{{ t("chatInput.attach.noTerminal") }}</div>
        <div
          v-for="(terminal, j) in terminals"
          :key="terminal.id"
          role="menuitem"
          :class="['mi', { hv: subActive === j }]"
          @mouseenter="subActive = j"
          @click="pickTerminal(j)"
        >
          <Icon name="terminal" :size="14" class="ic" />
          <span class="lb">{{ terminal.name }}</span>
        </div>
      </div>
    </template>
  </Teleport>
</template>

<style scoped>
.menu {
  position: fixed;
  z-index: 70;
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

.menu:focus-visible {
  outline: none;
}

.mi {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 30px;
  flex-shrink: 0;
  padding: 0 10px;
  border-radius: 6px;
  white-space: nowrap;
  cursor: default;
}

.mi.hv {
  background: var(--bg-lit);
}

.ic {
  flex-shrink: 0;
  color: var(--text-nav);
}

.ch {
  flex-shrink: 0;
  color: var(--text-muted);
}

.lb {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.none {
  padding: 7px 10px;
  color: var(--text-faint);
  white-space: nowrap;
}
</style>
