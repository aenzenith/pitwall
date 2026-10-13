<script setup lang="ts">
import { onMounted, reactive, ref, watch } from "vue";

import Spinner from "../../components/Spinner.vue";
import { language, t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";
import type { PhoneChannel, PhoneConfig, PhoneEvents, PhoneView, PhoneWhen } from "../../lib/types";

/**
 * Settings › Notifications: the pit board on the phone. The channel (ntfy, a Telegram bot or a
 * webhook) with its address, when, which events, and the very text that goes. Each change goes
 * to the core by its own command (`phone_set`), which keeps it in a file of its own; a Telegram
 * token is typed here and never comes back, only whether one is stored.
 */
const CHANNELS: Array<{ id: PhoneChannel; name: string }> = [
  { id: "ntfy", name: "ntfy" },
  { id: "telegram", name: "Telegram" },
  { id: "webhook", name: "Webhook" },
];
const WHEN: Record<PhoneWhen, Key> = { locked: "phone.when.locked", idle: "phone.when.idle", always: "phone.when.always" };
/** Claude's three down the first column, the rest down the second. */
const EVENTS: Array<{ id: keyof PhoneEvents; label: Key }> = [
  { id: "asked", label: "phone.event.asked" },
  { id: "permission", label: "phone.event.permission" },
  { id: "finished", label: "phone.event.finished" },
  { id: "crashed", label: "phone.event.crashed" },
  { id: "fuel", label: "phone.event.fuel" },
  { id: "card", label: "phone.event.card" },
];
/** The fields typed in (`data-phone`): the core's answer never writes over one in use. */
const TEXTS = ["ntfyServer", "ntfyTopic", "telegramChat", "webhookUrl"] as const;
/** Stands in the token's field for a token that is stored. */
const STORED = "•".repeat(24);

const form = reactive<PhoneConfig>({
  on: false,
  channel: "ntfy",
  ntfyServer: "",
  ntfyTopic: "",
  telegramToken: "",
  telegramChat: "",
  webhookUrl: "",
  when: "always",
  events: { asked: true, permission: true, finished: false, crashed: true, fuel: true, card: false },
});
/** The channel as the core holds it; null until it answered. */
const view = ref<PhoneView | null>(null);
/** A change the core couldn't keep. */
const error = ref("");
const testing = ref(false);
/** The last test's answer: sent, or why not. */
const answer = ref<{ ok: boolean; text: string } | null>(null);
/** Saves go to the core one after the other, and a test waits for the last. */
let saving: Promise<void> = Promise.resolve();

/** Takes the core's answer. After a save (`sent`: the form as it went) only the fields come in,
 *  as the core keeps them (trimmed, a fresh topic): the one being typed in and any edited since
 *  stay as they are. A token the core has kept leaves its field, so no later save carries it. */
function take(next: PhoneView, sent?: PhoneConfig): void {
  view.value = next;
  if (sent?.telegramToken && next.telegramToken && form.telegramToken === sent.telegramToken) form.telegramToken = "";
  const focused = (document.activeElement as HTMLElement | null)?.dataset?.phone;
  for (const key of TEXTS) {
    if (key === focused || (sent && form[key] !== sent[key])) continue;
    form[key] = next[key];
  }
  if (sent) return;
  form.on = next.on;
  form.channel = next.channel;
  form.when = next.when;
  form.events = { ...next.events };
}

function load(): void {
  void api.phoneConfig().then(
    (next) => next && take(next),
    () => undefined,
  );
}

function save(): Promise<void> {
  const sent: PhoneConfig = { ...form, events: { ...form.events } };
  answer.value = null;
  saving = saving.then(async () => {
    try {
      const kept = await api.phoneSet(sent);
      if (kept) take(kept, sent);
      error.value = "";
    } catch (reason) {
      error.value = String(reason);
    }
  });
  return saving;
}

function setOn(on: boolean): void {
  form.on = on;
  void save();
}

function setChannel(channel: PhoneChannel): void {
  if (form.channel === channel) return;
  form.channel = channel;
  void save();
}

function setWhen(when: PhoneWhen): void {
  if (form.when === when) return;
  form.when = when;
  void save();
}

function setEvent(id: keyof PhoneEvents, on: boolean): void {
  form.events[id] = on;
  void save();
}

async function newTopic(): Promise<void> {
  const topic = await api.phoneTopic().catch(() => null);
  if (!topic) return;
  form.ntfyTopic = topic;
  void save();
}

/** The test goes to the address on screen: what was just typed is kept first. */
async function test(): Promise<void> {
  if (testing.value) return;
  testing.value = true;
  await saving;
  let result: { ok: boolean; text: string };
  try {
    await api.phoneTest();
    result = { ok: true, text: t("phone.test.sent") };
  } catch (reason) {
    result = { ok: false, text: String(reason) };
  }
  answer.value = result;
  if (view.value) view.value.failure = result.ok ? null : result.text;
  testing.value = false;
}

onMounted(load);
// The text that goes is in the language spoken.
watch(language, load);
</script>

<template>
  <!-- A part of the Notifications tab, under its own label as the tab's other parts are. -->
  <div v-if="view" class="section phone" role="group" aria-labelledby="phone-section">
    <div id="phone-section" class="section-label">{{ t("phone.section") }}</div>
    <div class="rows">
      <label class="row">
        <span class="name">{{ t("phone.title") }}</span>
        <input type="checkbox" role="switch" :checked="form.on" @change="setOn(($event.target as HTMLInputElement).checked)" />
      </label>
    </div>

    <!-- Off, everything under the switch stays where it is, out of reach. -->
    <fieldset class="phone-rest" :disabled="!form.on">
      <div class="phone-parts">
        <div class="phone-part">
          <span id="phone-channel">{{ t("phone.channel") }}</span>
          <span class="seg" role="group" aria-labelledby="phone-channel">
            <button v-for="item in CHANNELS" :key="item.id" type="button" :aria-pressed="form.channel === item.id" @click="setChannel(item.id)">{{ item.name }}</button>
          </span>

          <div v-if="form.channel === 'ntfy'" class="phone-fields ntfy">
            <label class="phone-field">
              <span>{{ t("phone.ntfy.server") }}</span>
              <input v-model="form.ntfyServer" data-phone="ntfyServer" class="mono" spellcheck="false" autocomplete="off" autocapitalize="off" @change="save" />
            </label>
            <label class="phone-field">
              <span>{{ t("phone.ntfy.topic") }}</span>
              <input v-model="form.ntfyTopic" data-phone="ntfyTopic" class="mono" spellcheck="false" autocomplete="off" autocapitalize="off" @change="save" />
            </label>
            <button type="button" class="control" @click="newTopic">{{ t("phone.ntfy.newTopic") }}</button>
          </div>
          <div v-else-if="form.channel === 'telegram'" class="phone-fields telegram">
            <label class="phone-field">
              <span>{{ t("phone.telegram.token") }}</span>
              <input v-model="form.telegramToken" type="password" class="mono" spellcheck="false" autocomplete="off" :placeholder="view.telegramToken ? STORED : ''" @change="save" />
            </label>
            <label class="phone-field">
              <span>{{ t("phone.telegram.chat") }}</span>
              <input v-model="form.telegramChat" data-phone="telegramChat" class="mono" spellcheck="false" autocomplete="off" autocapitalize="off" @change="save" />
            </label>
          </div>
          <div v-else class="phone-fields">
            <label class="phone-field">
              <span>{{ t("phone.webhook.url") }}</span>
              <input v-model="form.webhookUrl" data-phone="webhookUrl" class="mono" spellcheck="false" autocomplete="off" autocapitalize="off" placeholder="https://" @change="save" />
            </label>
          </div>

          <div class="phone-test">
            <button type="button" class="control" :disabled="testing" :aria-busy="testing" @click="test">
              <!-- The label keeps its room under the spinner: the button never changes width. -->
              <span :class="{ hidden: testing }">{{ t("phone.test") }}</span>
              <Spinner v-if="testing" class="over" :size="12" />
            </button>
            <span v-if="answer" :class="['phone-answer', { bad: !answer.ok }]" :role="answer.ok ? 'status' : 'alert'">{{ answer.text }}</span>
            <span v-else-if="view.failure && !testing" class="phone-answer bad">{{ t("phone.failure", { error: view.failure }) }}</span>
          </div>
        </div>

        <!-- One moment alone is no choice. -->
        <div v-if="view.moments.length > 1" class="phone-part">
          <span id="phone-when">{{ t("phone.when") }}</span>
          <span class="seg" role="group" aria-labelledby="phone-when">
            <button v-for="moment in view.moments" :key="moment" type="button" :aria-pressed="form.when === moment" @click="setWhen(moment)">{{ t(WHEN[moment]) }}</button>
          </span>
        </div>

        <div class="phone-part">
          <span id="phone-events">{{ t("phone.events") }}</span>
          <div class="phone-events" role="group" aria-labelledby="phone-events">
            <label v-for="event in EVENTS" :key="event.id" class="row">
              <input type="checkbox" :checked="form.events[event.id]" @change="setEvent(event.id, ($event.target as HTMLInputElement).checked)" />
              <span class="name">{{ t(event.label) }}</span>
            </label>
          </div>
        </div>

        <div class="phone-part">
          <span>{{ t("phone.sent") }}</span>
          <div class="phone-sample">{{ view.sample }}</div>
          <span class="phone-note">{{ t("phone.sentNote") }}</span>
        </div>
      </div>
    </fieldset>

    <p v-if="error" class="error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped src="./settings.css"></style>
<style scoped>
/* What the part holds, as far apart as a tab's rows are; its label sits closer to what it names. */
.phone {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.phone > .section-label {
  margin-bottom: -6px;
}

/* The box is only there to turn its controls off; the layout is its child's (a fieldset is no
   flex container in every WebKit). */
.phone-rest {
  min-width: 0;
  margin: 0;
  padding: 0;
  border: 0;
  transition: opacity 0.15s;
}

.phone-rest:disabled {
  opacity: 0.45;
}

/* Out of reach as a whole: its buttons aren't dimmed a second time. */
.phone-rest:disabled .control {
  opacity: 1;
}

.phone-parts {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.phone-part {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}

/* The channel's address: its fields side by side, each under its name. Every channel's row is
   as high, so picking another moves nothing under it. */
.phone-fields {
  align-self: stretch;
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  align-items: end;
  gap: 8px;
}

.phone-fields.ntfy {
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
}

.phone-fields.telegram {
  grid-template-columns: minmax(0, 1fr) 150px;
}

.phone-field {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

/* One line's height whatever the script: a channel's row is as high in every language. */
.phone-field span {
  font-size: 12px;
  line-height: 16px;
  color: var(--text-subtle);
}

.phone-field input {
  width: 100%;
  min-width: 0;
  height: 32px;
  box-sizing: border-box;
  padding: 0 10px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.phone-field input.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}

/* The test and, beside it, its answer: a long one wraps there, and the row has two lines' room. */
.phone-test {
  align-self: stretch;
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 36px;
}

.phone-answer {
  min-width: 0;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}

.phone-answer.bad {
  color: var(--crash-text);
}

/* Two columns, filled downwards: a line a box, as the tab's own lists. */
.phone-events {
  align-self: stretch;
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-template-rows: repeat(3, auto);
  grid-auto-flow: column;
  column-gap: 20px;
  margin: -6px 0;
}

/* The first line of each column has none above it. */
.phone-events > .row:nth-child(3n + 1) {
  border-top: 0;
}

.phone-rest:disabled .row {
  cursor: default;
}

/* The text as it goes, in the face output is in. */
.phone-sample {
  align-self: stretch;
  min-height: 18px;
  padding: 8px 12px;
  border: 1px solid var(--line-inset);
  border-radius: var(--radius-control);
  background: var(--bg-log);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 18px;
  color: var(--text-log);
  overflow-wrap: anywhere;
}

.phone-note {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-subtle);
}
</style>
