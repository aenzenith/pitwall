<script setup lang="ts">
import { ref } from "vue";

import Icon from "../../components/Icon.vue";
import { MODE_ICONS, type PlanMode } from "../../lib/chat/ui";
import { t, type Key } from "../../lib/i18n";
import ChatIcon from "./ChatIcon.vue";
import { useChipMenu } from "./dialogs/shared";

/**
 * The modes a plan can start in, as a menu over the split button (PlanStart), in the mode chip's
 * dress (ModeDialog): each with what it does, the picked one checked, auto greyed with why when
 * the chat's model hasn't it. A click picks; nothing is answered from here.
 */
const props = defineProps<{ anchor: HTMLElement | null; modes: readonly PlanMode[]; mode: PlanMode; autoOn: boolean }>();
const emit = defineEmits<{ pick: [mode: PlanMode]; close: [] }>();

const NAMES: Record<PlanMode, Key> = {
  acceptEdits: "chatInput.plan.mode.acceptEdits",
  auto: "chatInput.plan.mode.auto",
  default: "chatInput.plan.mode.default",
};
const NOTES: Record<PlanMode, Key> = {
  acceptEdits: "chatDialog.mode.acceptEditsNote",
  auto: "chatDialog.mode.autoNote",
  default: "chatDialog.mode.defaultNote",
};

const menu = ref<HTMLElement | null>(null);
const WIDTH = 340;
const place = useChipMenu({ anchor: () => props.anchor, width: WIDTH, menu, focus: () => menu.value, close: () => emit("close") });

const off = (mode: PlanMode) => mode === "auto" && !props.autoOn;

function choose(mode: PlanMode): void {
  if (off(mode)) return;
  emit("pick", mode);
}

function onKey(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}
</script>

<template>
  <Teleport to="body">
    <div ref="menu" class="chip-menu" tabindex="-1" role="menu" :aria-label="t('chatInput.plan.modes')" :style="{ width: `${WIDTH}px`, ...place }" @keydown="onKey">
      <div class="list">
        <button
          v-for="each in modes"
          :key="each"
          type="button"
          role="menuitemradio"
          :aria-checked="each === mode"
          :aria-disabled="off(each) || undefined"
          :class="['row', 'tall', { off: off(each) }]"
          @click="choose(each)"
        >
          <span class="mark lead"><ChatIcon :name="MODE_ICONS[each]" :size="14" /></span>
          <span class="text">
            <span class="name">{{ t(NAMES[each]) }}</span>
            <span class="desc">{{ t(off(each) ? "chatDialog.mode.autoOff" : NOTES[each]) }}</span>
          </span>
          <span class="mark end"><Icon v-if="each === mode" name="check" :size="13" /></span>
        </button>
      </div>
    </div>
  </Teleport>
</template>

<style scoped src="./dialogs/dialog.css"></style>
<style scoped src="./dialogs/menu.css"></style>

<style scoped>
.row.tall {
  width: 100%;
  align-items: flex-start;
  min-height: 0;
  padding: 6px 10px;
  border: none;
  background: transparent;
  font: inherit;
  text-align: left;
  white-space: normal;
  color: inherit;
}

/* A button, so its ground is set again over the button's own: dialog.css's hover. */
.row.tall:hover:not(.off) {
  background: var(--bg-hover);
}

.row .name {
  font-weight: 500;
}

.row .desc {
  font-size: 11.5px;
}

.row.tall .mark.lead {
  margin-top: 2px;
  color: var(--text-nav);
}

.row.tall .mark.end {
  align-self: center;
}
</style>
