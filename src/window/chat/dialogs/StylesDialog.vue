<script setup lang="ts">
import { computed, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { projectName, send, useListNav, type DialogProps } from "./shared";

/**
 * Output styles (B9): the styles the CLI offers (`initialize.available_output_styles`), the
 * current one checked; a pick switches to it (`apply_flag_settings {outputStyle}`) and closes.
 * Claude Code's own styles get a sentence; the user's keep their names.
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

const NOTES: Record<string, Key> = {
  default: "chatDialog.styles.defaultNote",
  explanatory: "chatDialog.styles.explanatoryNote",
  learning: "chatDialog.styles.learningNote",
};

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);

const styles = computed(() => props.info?.outputStyles ?? []);
const chosen = ref<string | null>(null);
const current = computed(() => chosen.value ?? props.info?.outputStyle ?? "default");

const label = (style: string) => (style === "default" ? t("chatDialog.styles.default") : style);
const note = (style: string) => {
  const key = NOTES[style.toLowerCase()];
  return key ? t(key) : "";
};

const busy = ref<string | null>(null);
const error = ref<string | null>(null);

async function pick(style: string): Promise<void> {
  if (busy.value) return;
  if (style === current.value) return frame.value?.close();
  busy.value = style;
  error.value = await send(props.id, { kind: "setStyle", style });
  busy.value = null;
  if (error.value) return;
  chosen.value = style;
  frame.value?.close();
}

const nav = useListNav(
  () => styles.value.length,
  (i) => void pick(styles.value[i]),
  listEl,
);
const wanted = props.arg?.trim().toLowerCase();
nav.active.value = Math.max(
  0,
  styles.value.findIndex((style) => (wanted ? style.toLowerCase() === wanted : style === current.value)),
);

const rowId = (i: number) => `chat-style-${props.id}-${i}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.styles.title')" :sub="projectName(path)" :width="420" @close="emit('close')" @key="nav.onKey">
    <div v-if="!styles.length" class="state">
      <Spinner v-if="!info" :size="14" />
      <template v-else>{{ t("chatDialog.styles.empty") }}</template>
    </div>
    <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.styles.title')" :aria-activedescendant="rowId(nav.active.value)">
      <div
        v-for="(style, i) in styles"
        :id="rowId(i)"
        :key="style"
        :data-row="i"
        role="option"
        :aria-selected="i === nav.active.value"
        :aria-checked="style === current"
        :class="['row', { active: i === nav.active.value }]"
        @click="
          nav.active.value = i;
          pick(style);
        "
      >
        <span class="text">
          <span class="name">{{ label(style) }}</span>
          <span v-if="note(style)" class="desc">{{ note(style) }}</span>
        </span>
        <span class="mark end">
          <Spinner v-if="busy === style" :size="12" />
          <Icon v-else-if="style === current" name="check" :size="13" />
        </span>
      </div>
    </div>

    <NotAnswered v-if="error" :id="id" :state="state" :error="error" @done="frame?.close()" />
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>
