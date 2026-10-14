<script lang="ts">
import { ref } from "vue";

import type { CliState } from "../../lib/types";

/** What the core said when last asked: kept while the app runs, so the part opens as it was. */
const asked = ref<CliState | null>(null);
/** The ask under way: one at a time, however often the part comes into view. */
let flying: Promise<void> | null = null;
/** Counted up as an install or a removal begins and ends: an answer asked for before then is not taken. */
let turns = 0;
</script>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from "vue";

import InfoPopover from "../../components/InfoPopover.vue";
import Spinner from "../../components/Spinner.vue";
import { t, type Key } from "../../lib/i18n";
import { isMac } from "../../lib/platform";
import { api, snapshot } from "../../lib/store";

/**
 * Settings › General's command line: where the `pitwall` command is, with the button that
 * installs it or takes it out, and whether `pitwall://` links are answered. Its own: each
 * change goes to the core by its own command, never with the form (`@change.stop` around it).
 *
 * Where the command is only the core can tell (it looks through the shell's PATH): asked each
 * time the part comes into view and after a change, never on a timer. The links' switch is the
 * snapshot's.
 */

const links = computed(() => !!snapshot.value?.settings.cli?.links);
/** Where the command is; none while it isn't installed, or isn't known yet. */
const where = computed(() => asked.value?.path ?? null);
/** That path in two: its folders, which give way when it is long, and its end (the last folder and the command), which stays. */
const parts = computed(() => {
  const path = where.value ?? "";
  const cut = path.lastIndexOf("/", path.lastIndexOf("/") - 1);
  return cut > 0 ? { head: path.slice(0, cut), tail: path.slice(cut) } : { head: "", tail: path };
});
/** Being installed or taken out. */
const busy = ref(false);
const error = ref("");

/** On macOS only so far: nothing of it is drawn elsewhere. */
const hidden = computed(() => !isMac || asked.value?.status === "unsupported");

/**
 * The commands behind the label's (i), as `pitwall --help` gives them (cli.rs: `synopsis`, `help`)
 * with what each does in its words, and a line to run for each. The project those that act on one
 * take last is said once, under them.
 */
const commands = computed<Array<{ syntax: string; does: Key; examples: string[] }>>(() => {
  const arg = (key: Key): string => `<${t(key)}>`;
  return [
    { syntax: "status [--json]", does: "core.cli.help.status", examples: ["pitwall status"] },
    { syntax: "start | stop | restart", does: "core.cli.help.act", examples: ["pitwall restart web"] },
    { syntax: `logs [-f] [--command ${arg("core.cli.arg.name")}]`, does: "core.cli.help.logs", examples: ["pitwall logs -f", "pitwall logs --command test"] },
    { syntax: `run ${arg("core.cli.arg.name")}`, does: "core.cli.help.run", examples: ["pitwall run test"] },
    { syntax: `stop --command ${arg("core.cli.arg.name")}`, does: "core.cli.help.stopCommand", examples: ["pitwall stop --command test"] },
    { syntax: `card "${arg("core.cli.arg.title")}" [--note ${arg("core.cli.arg.text")}]`, does: "core.cli.help.card", examples: [`pitwall card "${t("cli.table.cardExample")}"`] },
    { syntax: "open", does: "core.cli.help.open", examples: ["pitwall open web"] },
    { syntax: "edit", does: "core.cli.help.edit", examples: ["pitwall edit"] },
    { syntax: "quiet <1h | morning | off>", does: "core.cli.help.quiet", examples: ["pitwall quiet morning"] },
    { syntax: "--help", does: "cli.table.help", examples: ["pitwall --help"] },
    { syntax: "--version", does: "cli.table.version", examples: ["pitwall --version"] },
  ];
});

function ask(): Promise<void> {
  flying ??= (async () => {
    const mine = turns;
    try {
      const state = await api.cliState();
      // An install or a removal meanwhile knows better.
      if (mine === turns) asked.value = state;
    } catch {
      // Kept as it was.
    }
  })().finally(() => (flying = null));
  return flying;
}

/** Installs the command, or takes it out when it is there. */
async function turn(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  turns++;
  try {
    asked.value = await (where.value ? api.cliRemove() : api.cliInstall());
  } catch (reason) {
    error.value = String(reason);
  } finally {
    turns++;
    busy.value = false;
  }
  if (!error.value) return;
  // How it stands now is asked, once an ask from before is out of the way.
  await flying;
  void ask();
}

async function setLinks(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  try {
    await api.cliLinks(box.checked);
  } catch {
    // Not taken: the switch shows what is stored again.
    box.checked = links.value;
  }
}

/* ---------- asked when it comes into view ---------- */

const root = ref<HTMLElement | null>(null);
let watcher: IntersectionObserver | null = null;

onMounted(() => {
  watcher = new IntersectionObserver((entries) => {
    if (!entries.some((entry) => entry.isIntersecting)) return;
    // Looked at anew: what went wrong the last time has been read.
    error.value = "";
    void ask();
  });
  if (root.value) watcher.observe(root.value);
});

onBeforeUnmount(() => watcher?.disconnect());
</script>

<template>
  <div v-if="!hidden" ref="root" class="section part cli">
    <div class="label-line">
      <div id="cli-section" class="section-label">{{ t("cli.section") }}</div>
      <InfoPopover :label="t('cli.commands')">
        <table class="commands">
          <thead>
            <tr>
              <th scope="col" class="section-label">{{ t("cli.table.command") }}</th>
              <th scope="col" class="section-label">{{ t("cli.table.does") }}</th>
              <th scope="col" class="section-label">{{ t("cli.table.example") }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in commands" :key="row.does">
              <th scope="row"><code>{{ row.syntax }}</code></th>
              <td><span class="does">{{ t(row.does) }}</span></td>
              <!-- One click takes the whole line, to copy (its menu's Copy too: `selectable`). -->
              <td class="example"><code v-for="line in row.examples" :key="line" class="selectable">{{ line }}</code></td>
            </tr>
          </tbody>
        </table>
        <p class="project"><code>[{{ t("core.cli.arg.project") }}]</code> {{ t("core.cli.help.project") }}</p>
      </InfoPopover>
    </div>
    <div class="rows" role="group" aria-labelledby="cli-section">
      <div class="row">
        <span class="name fixed">{{ t("cli.command") }}</span>
        <!-- Where it is, or that it isn't; nothing until the core has said. A long path is cut
             in its middle (its end, …/bin/pitwall, stays); the whole of it is the title. -->
        <span v-if="asked" class="where" :title="where ?? undefined">
          <template v-if="where"><span class="head">{{ parts.head }}</span><span class="tail">{{ parts.tail }}</span></template>
          <template v-else>{{ t("cli.missing") }}</template>
        </span>
        <button type="button" class="control" :disabled="busy || !asked" :aria-busy="busy" @click="turn">
          <!-- Both labels keep their room, one over the other: the button is one width installed
               or not, and under its spinner. -->
          <span :class="['labels', { hidden: busy }]">
            <span :class="{ away: !!where }" :aria-hidden="!!where">{{ t("cli.install") }}</span>
            <span :class="{ away: !where }" :aria-hidden="!where">{{ t("cli.remove") }}</span>
          </span>
          <Spinner v-if="busy" class="over" :size="12" />
        </button>
      </div>
      <!-- The whole row is the switch's. -->
      <label class="row">
        <span class="name">{{ t("cli.links") }}</span>
        <input type="checkbox" role="switch" :checked="links" @change="setLinks" />
      </label>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
.cli {
  font-size: 13px;
}

/* Under another part of the same box (the parts before it are components of their own): as far
   from it as a tab's parts are from each other. */
.cli:not(:first-child) {
  margin-top: 16px;
}

/* The command's row: its label keeps its one line, and the path takes what is left of the row. */
.name.fixed {
  flex: 0 0 auto;
  white-space: nowrap;
}

/* Where the command is: a path, in the face paths are in, at the button's side. Too long for
   the row, its folders give way and its end stays. */
.where {
  display: flex;
  flex: 1 1 0;
  justify-content: flex-end;
  min-width: 0;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-subtle);
}

.where .head {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.where .tail {
  flex: none;
}

/* Until the core has said where the command is there is no path: the button keeps the row's end. */
.row > .control {
  margin-left: auto;
}

/* In a row, a button is as tall as the row's other controls. */
.row .control {
  height: 26px;
  padding: 0 10px;
  font-size: 12px;
}

.labels {
  display: grid;
}

.labels > span {
  grid-area: 1 / 1;
}

.away {
  visibility: hidden;
}

/* The part's label, and the (i) with its commands beside it. */
.label-line {
  display: flex;
  align-items: center;
  gap: 6px;
}

/* The commands, in the popover: a command and a line to run stay whole, what it does wraps. */
.commands {
  border-collapse: collapse;
}

.commands th,
.commands td {
  padding: 5px 12px 5px 0;
  text-align: start;
  vertical-align: baseline;
}

.commands tr > :last-child {
  padding-right: 0;
}

.commands thead th {
  padding-top: 0;
  padding-bottom: 6px;
  white-space: nowrap;
}

.commands tbody tr {
  border-top: 1px solid var(--line-strong);
}

.commands tbody th {
  font-weight: 400;
}

/* Never narrower than this (a cell's own min-width is no rule for a table): Chinese and Japanese,
   which may break between any two characters, would stand a character to a line. */
.does {
  display: block;
  min-width: 15em;
}

.commands code,
.project code {
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: nowrap;
}

.commands tbody th code {
  color: var(--text-strong);
}

.example code {
  display: block;
  color: var(--text);
  -webkit-user-select: all;
  user-select: all;
}

.example code + code {
  margin-top: 3px;
}

/* The project they take, said once under them. */
.project {
  margin: 8px 0 0;
  color: var(--text-subtle);
}

.project code {
  color: var(--text-muted);
}
</style>
