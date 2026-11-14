<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { planAutoOn, planModeOf } from "../../lib/chat/plan";
import { MODE_ICONS, PLAN_MODES, type PlanMode } from "../../lib/chat/ui";
import { t } from "../../lib/i18n";
import ChatIcon from "./ChatIcon.vue";
import PlanModeMenu from "./PlanModeMenu.vue";

/**
 * Claude's plan approved (PL5, the user's pick of 18 Nov): "Onayla ve başla" starts in the mode
 * picked, the arrow beside it opens the three modes to pick from. A pick only picks; the button
 * starts. Clicks alone answer: no key picks a mode or approves (the user's word, 18 Nov).
 */
const props = defineProps<{ id: number; mode: PlanMode; sending: boolean; disabled?: boolean; label?: string }>();
const emit = defineEmits<{ start: [mode: PlanMode]; pick: [mode: PlanMode] }>();

const split = ref<HTMLElement | null>(null);
const open = ref(false);

const autoOn = computed(() => planAutoOn(props.id));
const mode = computed<PlanMode>(() => planModeOf(props.id, props.mode));

function pick(picked: PlanMode): void {
  open.value = false;
  emit("pick", picked);
}
</script>

<template>
  <span ref="split" class="split">
    <button type="button" class="cc-btn primary main" :disabled="disabled || sending" @click="emit('start', mode)">
      <Spinner v-if="sending" :size="11" />
      <ChatIcon v-else :name="MODE_ICONS[mode]" :size="13" />
      <span class="word">{{ label ?? t("chatInput.plan.start") }}</span>
    </button>
    <button
      type="button"
      class="cc-btn primary arrow"
      :disabled="disabled || sending"
      :aria-label="t('chatInput.plan.modes')"
      :title="t('chatInput.plan.modes')"
      aria-haspopup="menu"
      :aria-expanded="open"
      @click="open = !open"
    >
      <Icon name="chevron-down" :size="13" :class="['turn', { up: open }]" />
    </button>
    <PlanModeMenu v-if="open" :anchor="split" :modes="PLAN_MODES" :mode="mode" :auto-on="autoOn" @pick="pick" @close="open = false" />
  </span>
</template>

<style scoped>
.split {
  display: inline-flex;
  flex-shrink: 0;
  min-width: 0;
}

.split .main {
  min-width: 0;
  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
}

.split .word {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.split .arrow {
  padding: 0 7px;
  border-left-color: color-mix(in srgb, var(--btn-primary-text) 28%, transparent);
  border-top-left-radius: 0;
  border-bottom-left-radius: 0;
}

.turn {
  transition: transform 0.15s;
}

.turn.up {
  transform: rotate(180deg);
}
</style>
