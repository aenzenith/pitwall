<script setup lang="ts">
import { computed } from "vue";

import ClaudeSpark from "../../components/ClaudeSpark.vue";
import Icon from "../../components/Icon.vue";
import Markdown from "../../components/Markdown.vue";
import { clock, duration } from "../../lib/day";
import { ago, editorName } from "../../lib/format";
import { compactText, moneyText } from "../../lib/fuel";
import { language, t } from "../../lib/i18n";
import { dragRegion, isMac, isWindows } from "../../lib/platform";
import { endedAt, lastActive, modelsText, runsElsewhere, sessionTerminal, sessionTitle, unlinkedEditor } from "../../lib/sessions";
import { snapshot } from "../../lib/store";
import type { SessionRow, TerminalView } from "../../lib/types";
import OriginLabel from "./OriginLabel.vue";
import SessionDay from "./SessionDay.vue";
import SessionNotice from "./SessionNotice.vue";
import SessionTerminal from "./SessionTerminal.vue";

/**
 * One session: what it is doing, its day and its tokens, and under them its terminal when it runs
 * in one of Pitwall's, else what Claude last said in it. The Sessions page's details, and a
 * board card's once it has a session.
 * Claude's state reads as a project's Claude card does (detail/ClaudeCard), its one button the
 * same; bringing it up, opening or adding its project are the list's (double-click, Enter, the
 * context menu). `others`: the other sessions of its project that wait on you (seen is kept per
 * project); `error`: why adding its folder failed; `terminal`: the Pitwall terminal to show, as
 * the caller knows it (a board card's; null: none), left out: the one its origin names; `note`:
 * a board card's note, shown once its work is over; `lastMessage`: what Claude last said in it
 * (Markdown), shown in the terminal's place. `hold`: the keyboard is in its terminal.
 */
const props = defineProps<{ row: SessionRow; now: number; others: number; error: string; terminal?: TerminalView | null; lastMessage?: string }>();
const emit = defineEmits<{ markSeen: []; hold: [on: boolean] }>();

const title = computed(() => sessionTitle(props.row));
/** Only Pitwall's own terminals show here; a session running anywhere else stays a summary. */
const ownTerminal = computed(() => (props.terminal === undefined ? sessionTerminal(props.row, snapshot.value?.projects ?? []) : props.terminal));
const waiting = computed(() => props.row.phase === "waiting");
const permission = computed(() => waiting.value && props.row.turn?.kind === "permission");
const elsewhere = computed(() => runsElsewhere(props.row));
/** Where it runs isn't known, and it runs (an ended one reopens in the editor either way). */
const unknown = computed(() => props.row.origin.kind === "unknown" && props.row.phase !== "ended");

const spark = computed(() => (waiting.value ? "waiting" : props.row.phase === "working" ? "working" : "quiet"));

const cardTitle = computed(() => {
  const row = props.row;
  switch (row.phase) {
    case "waiting": {
      const kind = row.turn?.kind;
      if (kind === "permission") return row.tool ? t("sessions.card.permissionTool", { tool: row.tool }) : t("detail.claudePermission");
      if (kind === "asking") return t("detail.claudeAsking");
      return kind === "finished" ? t("detail.claudeFinished") : t("sessions.status.waiting");
    }
    case "working":
      return t("detail.claudeWorking");
    case "idle":
      return t("sessions.card.idle");
    case "ended":
      return t("sessions.card.ended");
  }
});

const cardSub = computed(() => {
  const row = props.row;
  const now = props.now;
  switch (row.phase) {
    case "waiting": {
      const at = row.turn?.at ?? row.since;
      if (!row.turn || !at) return "";
      return row.turn.kind === "finished" ? t("detail.notLookedAt", { ago: ago(at, now) }) : t("sessions.card.waitingFor", { duration: duration(Math.max(now - at, 1)) });
    }
    case "working":
      return row.since ? t("sessions.card.workingFor", { duration: duration(Math.max(now - row.since, 1)) }) : t("detail.workingHint");
    case "idle": {
      const at = lastActive(row);
      return at ? t("sessions.card.lastActive", { ago: ago(at, now) }) : "";
    }
    case "ended": {
      const at = endedAt(row);
      return at ? t("sessions.card.endedAt", { time: clock(at) }) : "";
    }
  }
});

const seenHint = computed(() => (props.others > 0 ? t("sessions.seenOthers", { count: props.others, project: props.row.project }) : ""));

/** Why it can't be brought up: an editor window without the extension, or another app. */
const elsewhereNotice = computed(() => {
  const origin = props.row.origin;
  if (unlinkedEditor(origin)) {
    return { tag: t("sessions.unlinked.tag"), title: t("sessions.unlinked.title", { editor: editorName(snapshot.value?.settings.editor) }), body: t("sessions.unlinked.body") };
  }
  const title = origin.kind === "terminal" && origin.app ? t("sessions.elsewhere.title", { app: origin.app }) : t("sessions.elsewhere.titleUnnamed");
  return { tag: t("sessions.elsewhere.tag"), title, body: t("sessions.elsewhere.body") };
});

const system = isWindows ? "Windows" : "Linux";

/* ---------- fuel ---------- */

const spend = computed(() => (props.row.spend && props.row.spend.tokens.total > 0 ? props.row.spend : null));

const fuel = computed(() => {
  const s = spend.value;
  if (!s) return null;
  const lang = language.value;
  const count = (n: number): string => compactText(n, lang);
  return {
    models: modelsText(s) || "—",
    total: s.costUsd > 0 || s.priced ? t("sessions.fuel.total", { tokens: count(s.tokens.total), cost: moneyText(s.costUsd, "USD", lang) }) : t("sessions.fuel.tokens", { value: count(s.tokens.total) }),
    breakdown: t("sessions.fuel.breakdown", { input: count(s.tokens.input), output: count(s.tokens.output), write: count(s.tokens.cacheWrite), read: count(s.tokens.cacheRead) }),
  };
});
</script>

<template>
  <section class="detail" :aria-label="t('sessions.details')">
    <!-- 100px: its line meets the line under the list's filter bar.
         The name and lines under it drag the window. -->
    <header class="head" :data-tauri-drag-region="dragRegion">
      <div class="name" :title="title">{{ title }}</div>
      <div class="where">
        <span class="project">{{ row.project }}</span>
        <span v-if="!row.path" class="outside">{{ t("sessions.outside") }}</span>
        <span class="dot" aria-hidden="true">·</span>
        <OriginLabel :origin="row.origin" :project="row.project" long />
      </div>
      <div class="path" :title="row.folder">{{ row.folder }}</div>
    </header>

    <div class="body">
      <section class="block" :aria-label="t('window.col.claude')">
        <div class="section-label">{{ t("window.col.claude") }}</div>
        <div :class="['card', { hot: waiting }]">
          <ClaudeSpark :state="spark" :lock="permission" />
          <div class="card-text">
            <span class="card-title">{{ cardTitle }}</span>
            <span v-if="cardSub" class="card-sub">{{ cardSub }}</span>
          </div>
          <button
            v-if="waiting"
            type="button"
            class="seen"
            :title="t('sessions.seenTitle', { project: row.project })"
            :aria-describedby="seenHint ? 'session-seen-hint' : undefined"
            @click="emit('markSeen')"
          >
            {{ t("detail.markSeen") }}
          </button>
        </div>
        <p v-if="seenHint" id="session-seen-hint" class="hint">{{ seenHint }}</p>
        <p v-if="error" class="error" role="alert">{{ error }}</p>

        <SessionNotice v-if="elsewhere" compact icon="info" :tag="elsewhereNotice.tag" :title="elsewhereNotice.title">{{ elsewhereNotice.body }}</SessionNotice>
        <template v-else-if="unknown">
          <SessionNotice v-if="!isMac" compact icon="info" :tag="t('sessions.limits.tag', { system })" :title="t('sessions.limits.title')">{{ t("sessions.limits.body") }}</SessionNotice>
          <SessionNotice v-else compact icon="info" :tag="t('sessions.unknown.tag')" :title="t('sessions.unknown.title')">{{ t("sessions.unknown.body") }}</SessionNotice>
        </template>
      </section>

      <!-- Today's hours are kept for the listed projects only. -->
      <SessionDay v-if="row.today" :row="row" :now="now" />

      <section class="block" :aria-label="t('fuel.title')">
        <div class="section-label">{{ t("fuel.title") }}</div>
        <template v-if="fuel">
          <div class="fuel-line">
            <span class="models" :title="fuel.models">{{ fuel.models }}</span>
            <span class="mono">{{ fuel.total }}</span>
          </div>
          <span class="note">{{ fuel.breakdown }}</span>
          <span class="note">{{ t("sessions.fuel.api") }}</span>
          <span v-if="spend && !spend.priced" class="note">{{ t("sessions.fuel.unpriced") }}</span>
        </template>
        <span v-else class="note">{{ t("sessions.fuel.none") }}</span>
      </section>

      <!-- Its terminal shows the conversation itself, so the line on what is shown of it goes.
           In its place, what Claude last said. -->
      <SessionTerminal v-if="ownTerminal" :terminal="ownTerminal" @hold="emit('hold', $event)" />
      <template v-else>
        <section v-if="lastMessage" class="block" :aria-label="t('sessions.lastMessage')">
          <div class="section-label">{{ t("sessions.lastMessage") }}</div>
          <Markdown :text="lastMessage" />
        </section>
        <p class="privacy"><Icon name="lock" :size="12" />{{ t("sessions.privacy") }}</p>
      </template>
    </div>
  </section>
</template>

<style scoped src="../detail/detail.css"></style>
<style scoped src="./panel.css"></style>
<style scoped>
.outside {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 5px;
  background: var(--bg-control);
  font-size: 11px;
  line-height: 16px;
  color: var(--text-muted);
}

.where .origin {
  overflow: hidden;
}

/* ---------- Claude's state ---------- */

.card {
  display: flex;
  align-items: center;
  gap: 12px;
}

.card-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex-grow: 1;
  min-width: 0;
}

.card-title {
  font-size: 13px;
  font-weight: 500;
}

.card.hot .card-title {
  font-weight: 600;
  color: var(--claude-text);
}

.card-sub {
  font-size: 12px;
  color: var(--text-muted);
}

.card.hot .card-sub {
  color: #c9a88c;
}

/* Waiting on you: the project card's button (detail/ClaudeCard), filled in Claude's colour. */
.seen {
  flex-shrink: 0;
  height: 28px;
  padding: 0 12px;
  border: 0;
  border-radius: var(--radius-control);
  background: var(--claude);
  color: #1a110a;
  font-size: 12px;
  font-weight: 600;
}

.seen:hover {
  background: var(--claude-text);
}

.hint,
.error {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.error {
  color: var(--crash-text);
}

/* ---------- fuel ---------- */

.fuel-line {
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
  font-size: 13px;
}

.models {
  flex-grow: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mono {
  flex-shrink: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  white-space: nowrap;
}

.note {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.privacy {
  flex-shrink: 0;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  margin: 14px 20px 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}

.privacy svg {
  flex-shrink: 0;
  margin-top: 3px;
}
</style>
