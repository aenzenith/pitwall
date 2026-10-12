<script setup lang="ts">
import { computed, reactive, watch } from "vue";

import Icon from "../components/Icon.vue";
import Spinner from "../components/Spinner.vue";
import { t } from "../lib/i18n";
import { api, snapshot } from "../lib/store";
import type { Candidate } from "../lib/types";

/**
 * Projects that newly appeared in the projects folder (`snapshot.suggestions`), offered under the
 * project list: a quiet strip, a row each with "Add" and "Ignore". Nothing at all while there is
 * none. At most three rows; the rest is a count. The list's panel is narrow in a small window
 * (288 px in the smallest): the row then keeps the name and its buttons, as words, then as icons.
 */
const SHOWN = 3;

/** "New project: {name}" round the name, so a narrow row can keep the name alone. */
const lead = computed(() => {
  const [before = "", after = ""] = t("suggest.new").split("{name}");
  return { before, after };
});

/** Rows being answered: which button, until the core has done it. */
const busy = reactive(new Map<string, "add" | "dismiss">());
/** Why a row's answer didn't go through. */
const failed = reactive(new Map<string, string>());
/** Rows answered: gone at once, whether or not the next snapshot has dropped them yet. */
const answered = reactive(new Set<string>());

const open = computed(() => (snapshot.value?.suggestions ?? []).filter((candidate) => !answered.has(candidate.path)));
const shown = computed(() => open.value.slice(0, SHOWN));
const more = computed(() => open.value.length - shown.value.length);

// What the core no longer offers is forgotten here too: offered again later, it shows again.
watch(
  () => snapshot.value?.suggestions,
  (suggestions) => {
    const offered = new Set((suggestions ?? []).map((candidate) => candidate.path));
    for (const path of [...answered]) if (!offered.has(path)) answered.delete(path);
    for (const path of [...failed.keys()]) if (!offered.has(path)) failed.delete(path);
  },
);

async function answer(candidate: Candidate, what: "add" | "dismiss"): Promise<void> {
  const path = candidate.path;
  if (busy.has(path)) return;
  busy.set(path, what);
  failed.delete(path);
  try {
    await (what === "add" ? api.addProjects([path]) : api.dismissSuggestion(path));
    answered.add(path);
  } catch (reason) {
    failed.set(path, String(reason));
  } finally {
    busy.delete(path);
  }
}
</script>

<template>
  <section v-if="shown.length" class="suggest" :aria-label="t('suggest.label')">
    <div v-for="candidate in shown" :key="candidate.path" class="row">
      <Icon name="folder" :size="14" />
      <span class="what" :title="candidate.path">
        <span class="lead">{{ lead.before }}</span><span class="name">{{ candidate.name }}</span><span class="lead">{{ lead.after }}</span>
      </span>
      <span v-if="failed.has(candidate.path)" class="why" role="alert" :title="failed.get(candidate.path)">{{ failed.get(candidate.path) }}</span>
      <span v-else class="kind">{{ candidate.kind }}</span>
      <button
        type="button"
        class="act add"
        :disabled="busy.has(candidate.path)"
        :aria-busy="busy.get(candidate.path) === 'add'"
        :aria-label="t('suggest.addName', { name: candidate.name })"
        :title="t('suggest.add')"
        @click="answer(candidate, 'add')"
      >
        <span :class="['label', { hidden: busy.get(candidate.path) === 'add' }]">{{ t("suggest.add") }}</span>
        <Icon name="plus" :size="13" :class="['glyph', { hidden: busy.get(candidate.path) === 'add' }]" />
        <Spinner v-if="busy.get(candidate.path) === 'add'" class="over" :size="11" />
      </button>
      <button
        type="button"
        class="act"
        :disabled="busy.has(candidate.path)"
        :aria-busy="busy.get(candidate.path) === 'dismiss'"
        :aria-label="t('suggest.dismissName', { name: candidate.name })"
        :title="t('suggest.dismiss')"
        @click="answer(candidate, 'dismiss')"
      >
        <span :class="['label', { hidden: busy.get(candidate.path) === 'dismiss' }]">{{ t("suggest.dismiss") }}</span>
        <Icon name="close" :size="13" :class="['glyph', { hidden: busy.get(candidate.path) === 'dismiss' }]" />
        <Spinner v-if="busy.get(candidate.path) === 'dismiss'" class="over" :size="11" />
      </button>
    </div>
    <div v-if="more > 0" class="more">{{ t("suggest.more", { count: more }) }}</div>
  </section>
</template>

<style scoped>
/* Under the project table, over the terminals: a hairline, then a line for each find. Level with
   the table's rows (20 px from the panel's edges). */
.suggest {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  padding: 6px 20px;
  border-top: 1px solid var(--line);
  container-type: inline-size;
}

.row {
  height: 30px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-muted);
}

.row > svg {
  flex-shrink: 0;
  color: var(--text-subtle);
}

/* The name gives way before the buttons do. */
.what {
  flex: 0 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.name {
  font-weight: 500;
  color: var(--text);
}

/* What it is built with, quieter; it fills the row, so the buttons stand at its right end. */
.kind,
.why {
  flex: 1 100 0;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-subtle);
}

.why {
  color: var(--crash-text);
}

/* Small buttons, a line's height: "Add" in a box, "Ignore" without one until pointed at. */
.act {
  position: relative;
  flex-shrink: 0;
  height: 24px;
  padding: 0 9px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  font-size: 12px;
  color: var(--text-muted);
}

.act:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text-strong);
}

/* At the row's right end, also where nothing fills the row before it. */
.act.add {
  margin-left: auto;
  border-color: var(--line-strong);
  background: var(--bg-control);
  color: var(--text);
}

.act.add:hover:not(:disabled) {
  background: var(--bg-control-hover);
}

.act:disabled:not([aria-busy="true"]) {
  opacity: 0.4;
}

.act .hidden {
  visibility: hidden;
}

.act .over {
  position: absolute;
  inset: 0;
  margin: auto;
}

.more {
  padding: 2px 0 4px 22px;
  font-size: 12px;
  color: var(--text-subtle);
}

/* The buttons' icons, for the narrowest panel only. */
.glyph {
  display: none;
}

/* A narrow panel: the name alone, then the buttons. */
@container (max-width: 460px) {
  .lead,
  .kind {
    display: none;
  }
}

/* The narrowest: the buttons as icons (their names are in their tips), so the name keeps its room. */
@container (max-width: 340px) {
  .label {
    display: none;
  }

  .glyph {
    display: block;
  }

  .act {
    width: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }
}
</style>
