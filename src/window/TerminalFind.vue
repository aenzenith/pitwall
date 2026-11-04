<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";

import Icon from "../components/Icon.vue";
import { t } from "../lib/i18n";
import { isMac, keys } from "../lib/platform";
import { closeFind, FIND_LIMIT, findKey, findOptions, findQuery, finds, findStep, toggleFindOption } from "../lib/terminals";

/**
 * A terminal's search bar, over its top right corner (lib/terminals: `finds`): what is looked
 * for, its choices (match case, whole word, regular expression), which match of how many, the
 * steps up and down and the close. As in VS Code's terminal, Return steps up, to older lines, and
 * ⇧Return down; Esc closes it, the keyboard going back to the terminal.
 */
const props = defineProps<{ id: number }>();

const find = computed(() => finds.get(props.id));
const field = ref<HTMLInputElement | null>(null);

type Option = keyof typeof findOptions;
const OPTIONS: Array<{ key: Option; icon: "case" | "word" | "regex"; label: "terminal.findCase" | "terminal.findWord" | "terminal.findRegex"; code: string }> = [
  { key: "caseSensitive", icon: "case", label: "terminal.findCase", code: "KeyC" },
  { key: "wholeWord", icon: "word", label: "terminal.findWord", code: "KeyW" },
  { key: "regex", icon: "regex", label: "terminal.findRegex", code: "KeyR" },
];

/** A choice's keys in the field, as VS Code has them: ⌥⌘C, ⌥⌘W, ⌥⌘R; Alt+C, Alt+W, Alt+R. */
function optionOf(event: KeyboardEvent): Option | null {
  const held = isMac ? event.altKey && event.metaKey && !event.ctrlKey : event.altKey && !event.ctrlKey && !event.metaKey;
  if (!held || event.shiftKey) return null;
  return OPTIONS.find((option) => option.code === event.code)?.key ?? null;
}

function optionTitle(label: (typeof OPTIONS)[number]["label"], code: string): string {
  return `${t(label)} (${keys(isMac ? `alt+mod+${code}` : `alt+${code}`)})`;
}

/** Which match of how many: past the counted ones, "1000+", and "?" for the one it stands on. */
const count = computed(() => {
  const now = find.value;
  if (!now?.query) return "";
  if (now.invalid) return t("terminal.findInvalid");
  if (!now.count) return t("files.findNone");
  const total = now.count >= FIND_LIMIT ? `${FIND_LIMIT}+` : String(now.count);
  return t("files.findCount", { current: now.index < 0 ? "?" : now.index + 1, total });
});

/** The widest the count can read, so the bar keeps one width whatever it says. */
const widest = computed(() => [t("files.findCount", { current: FIND_LIMIT - 1, total: `${FIND_LIMIT}+` }), t("files.findNone"), t("terminal.findInvalid")]);

function onKey(event: KeyboardEvent): void {
  if (event.isComposing) return;
  const option = optionOf(event);
  const act = findKey(event);
  if (event.key === "Enter" && !event.metaKey && !event.ctrlKey && !event.altKey) findStep(props.id, event.shiftKey ? "next" : "previous");
  else if (event.key === "Escape") closeFind(props.id);
  else if (option) toggleFindOption(option);
  else if (act === "open") field.value?.select();
  else if (act) findStep(props.id, act);
  else return;
  event.preventDefault();
  event.stopPropagation();
}

/** ⌘F asked for the field: the keyboard to it, what it holds selected. */
function take(): void {
  const now = find.value;
  if (!now?.focus) return;
  now.focus = false;
  void nextTick(() => {
    field.value?.focus();
    field.value?.select();
  });
}

onMounted(take);
watch(() => find.value?.focus, take);
</script>

<template>
  <div v-if="find" :class="['term-find', { invalid: find.invalid }]" role="search">
    <Icon name="search" :size="13" />
    <input
      ref="field"
      type="text"
      :value="find.query"
      :placeholder="t('terminal.find')"
      :aria-label="t('terminal.find')"
      :aria-invalid="find.invalid"
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      autocorrect="off"
      @input="findQuery(id, ($event.target as HTMLInputElement).value)"
      @keydown="onKey"
    />
    <button
      v-for="option in OPTIONS"
      :key="option.key"
      type="button"
      :class="['option', { on: findOptions[option.key] }]"
      :aria-pressed="findOptions[option.key]"
      :aria-label="t(option.label)"
      :title="optionTitle(option.label, option.code)"
      @mousedown.prevent
      @click="toggleFindOption(option.key)"
    >
      <Icon :name="option.icon" :size="14" />
    </button>
    <span class="count" aria-live="polite">
      <span v-for="text in widest" :key="text" class="sizer" aria-hidden="true">{{ text }}</span>
      <span>{{ count }}</span>
    </span>
    <button
      type="button"
      class="step"
      :disabled="!find.count"
      :aria-label="t('output.searchPrevious')"
      :title="`${t('output.searchPrevious')} (${keys('Enter')})`"
      @mousedown.prevent
      @click="findStep(id, 'previous')"
    >
      <Icon name="chevron-down" :size="13" class="up" />
    </button>
    <button
      type="button"
      class="step"
      :disabled="!find.count"
      :aria-label="t('output.searchNext')"
      :title="`${t('output.searchNext')} (${keys('shift+Enter')})`"
      @mousedown.prevent
      @click="findStep(id, 'next')"
    >
      <Icon name="chevron-down" :size="13" />
    </button>
    <button type="button" class="step" :aria-label="t('output.searchClose')" :title="`${t('output.searchClose')} (${keys('Escape')})`" @click="closeFind(id)">
      <Icon name="close" :size="13" />
    </button>
  </div>
</template>

<style scoped>
/* Over the terminal's top right corner, clear of its scrollbar (and the marks beside it); where
   exactly, the panel says (TerminalPanel, TerminalPanes). */
.term-find {
  position: absolute;
  z-index: 10;
  top: 10px;
  right: 22px;
  width: 360px;
  max-width: calc(100% - 32px);
  height: 30px;
  box-sizing: border-box;
  padding: 0 3px 0 9px;
  display: flex;
  align-items: center;
  gap: 2px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-dialog);
  box-shadow: var(--shadow-menu);
  color: var(--text-subtle);
}

.term-find:focus-within {
  border-color: var(--line-picked);
}

.term-find.invalid,
.term-find.invalid:focus-within {
  border-color: var(--crash-line);
}

input {
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  margin-left: 4px;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  font: inherit;
  font-size: 13px;
  color: var(--text);
}

.option,
.step {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-subtle);
}

.option:hover,
.step:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

.option.on {
  background: var(--bg-control-on);
  color: var(--text-strong);
}

.step:disabled {
  opacity: 0.45;
}

.step .up {
  transform: rotate(180deg);
}

/* Every text it can read in one cell: the widest sets its width. */
.count {
  flex-shrink: 0;
  display: grid;
  justify-items: end;
  padding: 0 4px 0 6px;
  font-family: var(--font-mono);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.count > * {
  grid-area: 1 / 1;
}

.count > .sizer {
  visibility: hidden;
}

.invalid .count {
  color: var(--crash-text);
}
</style>
