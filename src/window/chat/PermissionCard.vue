<script setup lang="ts">
import { computed, nextTick, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import { alwaysOffer, field, inputText, lineDiff, mcpTool } from "../../lib/chat/cards";
import { relativePath } from "../../lib/chat/composer";
import type { CanUseToolRequest, ChatAnswer, PermissionDestination } from "../../lib/chat/wire";
import { t, type Key } from "../../lib/i18n";
import { api } from "../../lib/store";

/**
 * A tool's permission (B2; V143–V148): what Claude wants to run, write or fetch, and the answers
 * the CLI's own dialog has: Yes; Yes and don't ask again (only the CLI's own suggestions, to the
 * place picked); No, with words for Claude; Stop. 1/2/3 pick, Enter answers the one the keys rest
 * on, ←/→ move the place, Esc stops. `requires_user_interaction` takes the one-key yes away: the
 * keys only point, a click or the button's own key answers. The `compact` form is a row of buttons
 * for the Komutlar row.
 */
const props = defineProps<{ id: number; path: string; request: CanUseToolRequest; compact: boolean }>();
const emit = defineEmits<{ answer: [answer: ChatAnswer]; refocus: [] }>();

const input = computed<Record<string, unknown>>(() => (props.request.input && typeof props.request.input === "object" ? props.request.input : {}));
const tool = computed(() => props.request.tool_name);
const mcp = computed(() => mcpTool(tool.value));
const offer = computed(() => alwaysOffer(props.request));
const strict = computed(() => !!props.request.requires_user_interaction);

/* ---------- what it shows ---------- */

const file = computed(() => relativePath(field(input.value, "file_path") || field(input.value, "notebook_path"), props.path));

/** Edit and Write can be changed before they are allowed ("edit first", V148). */
const editKey = computed<"new_string" | "content" | null>(() => (tool.value === "Edit" ? "new_string" : tool.value === "Write" ? "content" : null));
const original = computed(() => (editKey.value ? field(input.value, editKey.value) : ""));
const editing = ref(false);
/** The words as the user left them: kept when the field closes (Esc, "show the diff"), sent with
 * the yes whenever they differ from Claude's. */
const edited = ref("");
const touched = ref(false);
const now = computed(() => (touched.value ? edited.value : original.value));

const diff = computed(() => {
  if (tool.value === "Edit") return lineDiff(field(input.value, "old_string"), now.value);
  if (tool.value === "Write") return lineDiff("", now.value);
  return [];
});

type Edit = { old: string; new: string };
const edits = computed<Edit[]>(() => {
  const list = input.value.edits;
  if (!Array.isArray(list)) return [];
  return list.flatMap((each) => (each && typeof each === "object" ? [{ old: String((each as Record<string, unknown>).old_string ?? ""), new: String((each as Record<string, unknown>).new_string ?? "") }] : []));
});

/** Of a tool the card has no view for: the one field that says most, shown above its input. */
const headline = computed(() => {
  for (const key of ["file_path", "path", "notebook_path", "url", "pattern", "query", "command", "skill", "description"]) {
    const value = field(input.value, key);
    if (value) return key.endsWith("path") ? relativePath(value, props.path) : value;
  }
  return "";
});
const showInput = ref(false);
const inputJson = computed(() => (showInput.value ? inputText(input.value) : ""));

const bashNote = computed(() => (tool.value === "Bash" ? field(input.value, "description") : ""));
/** The request's own description, unless it only says again what the card shows (a Write's
 * description is its file's name). */
const description = computed(() => {
  const text = (props.request.description ?? "").trim();
  const name = file.value.slice(file.value.lastIndexOf("/") + 1);
  return text && text !== bashNote.value && text !== props.request.title && text !== file.value && text !== name ? text : "";
});

const MODE_KEYS: Record<string, Key> = {
  acceptEdits: "chatInput.mode.acceptEdits",
  plan: "chatInput.mode.plan",
  auto: "chatInput.mode.auto",
  default: "chatInput.mode.default",
  dontAsk: "chatInput.mode.dontAsk",
};

const PLACES: Record<PermissionDestination, Key> = {
  localSettings: "chatInput.perm.place.localSettings",
  projectSettings: "chatInput.perm.place.projectSettings",
  userSettings: "chatInput.perm.place.userSettings",
  session: "chatInput.perm.place.session",
  cliArg: "chatInput.perm.place.session",
};

const alwaysLabel = computed(() => {
  const mode = offer.value?.mode;
  if (mode === "acceptEdits") return t("chatInput.perm.alwaysEdits");
  if (mode) return t("chatInput.perm.alwaysMode", { mode: t(MODE_KEYS[mode] ?? "chatInput.mode.default") });
  return props.compact ? t("chatInput.perm.alwaysSession") : t("chatInput.perm.always");
});

/* ---------- answering ---------- */

type Choice = "yes" | "always" | "no";
const choices = computed<Choice[]>(() => (offer.value ? ["yes", "always", "no"] : ["yes", "no"]));
const active = ref<Choice>(props.request.default_to_no ? "no" : "yes");
const place = ref<PermissionDestination>(offer.value?.initial ?? "localSettings");
const telling = ref(false);
const message = ref("");
const tellField = ref<HTMLInputElement | null>(null);
const sent = ref<Choice | "stop" | null>(null);

function editedInput(): Record<string, unknown> | undefined {
  if (!editKey.value || !touched.value || edited.value === original.value) return undefined;
  return { [editKey.value]: edited.value };
}

function send(answer: ChatAnswer, which: Choice | "stop"): void {
  if (sent.value) return;
  sent.value = which;
  emit("answer", answer);
}

function choose(choice: Choice): void {
  active.value = choice;
  if (choice === "yes") return send({ kind: "allow", input: editedInput() }, "yes");
  if (choice === "always" && offer.value) {
    const destination = props.compact ? "session" : place.value;
    return send({ kind: "allow", input: editedInput(), always: { suggestions: offer.value.suggestions, destination } }, "always");
  }
  if (props.compact) return send({ kind: "deny", message: "", interrupt: false }, "no");
  telling.value = true;
  void nextTick(() => tellField.value?.focus());
}

function sendNo(): void {
  send({ kind: "deny", message: message.value.trim(), interrupt: false }, "no");
}

function stop(): void {
  send({ kind: "deny", message: "", interrupt: true }, "stop");
}

function startEdit(): void {
  if (!touched.value) {
    edited.value = original.value;
    touched.value = true;
  }
  editing.value = !editing.value;
}

function movePlace(delta: number): void {
  const places = offer.value?.places ?? [];
  if (places.length < 2) return;
  const at = places.indexOf(place.value);
  place.value = places[(at + delta + places.length) % places.length];
}

function openAddress(): void {
  const url = field(input.value, "url");
  if (/^https?:\/\//i.test(url)) void api.openUrl(url);
}

/** The card's keys (the frame passes them on); true when one was used. */
function key(event: KeyboardEvent): boolean {
  if (sent.value) return false;
  const target = event.target;
  const writing = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
  if (writing) {
    if (event.key === "Escape") {
      if (target === tellField.value) telling.value = false;
      else editing.value = false;
      emit("refocus");
      return true;
    }
    if (event.key === "Enter" && target === tellField.value && !event.shiftKey && !event.isComposing) {
      sendNo();
      return true;
    }
    return false;
  }
  if (event.metaKey || event.ctrlKey || event.altKey) return false;
  const digit = Number(event.key);
  if (Number.isInteger(digit) && digit >= 1 && digit <= choices.value.length) {
    const choice = choices.value[digit - 1];
    if (strict.value) active.value = choice;
    else choose(choice);
    return true;
  }
  switch (event.key) {
    case "ArrowUp":
    case "ArrowDown": {
      const at = choices.value.indexOf(active.value);
      active.value = choices.value[(at + (event.key === "ArrowDown" ? 1 : -1) + choices.value.length) % choices.value.length];
      return true;
    }
    case "ArrowLeft":
    case "ArrowRight":
      if (active.value !== "always") return false;
      movePlace(event.key === "ArrowRight" ? 1 : -1);
      return true;
    case "Enter":
      // A focused button answers by its own key; the card's Enter is the one-key yes.
      if (target instanceof HTMLButtonElement) return false;
      if (!strict.value) choose(active.value);
      return true;
    case "Escape":
      if (telling.value) {
        telling.value = false;
        return true;
      }
      stop();
      return true;
  }
  return false;
}

defineExpose({ key });
</script>

<template>
  <div v-if="compact" class="cc-actions compact" role="group" :aria-label="t('chatInput.card.permission', { tool: request.display_name || tool })">
    <button class="cc-btn sm primary" type="button" :disabled="!!sent" @click="choose('yes')"><Spinner v-if="sent === 'yes'" :size="11" />{{ t("chatInput.perm.yes") }}</button>
    <button v-if="offer" class="cc-btn sm" type="button" :disabled="!!sent" :title="[...offer.rules, ...offer.directories].join('\n') || undefined" @click="choose('always')"><Spinner v-if="sent === 'always'" :size="11" />{{ alwaysLabel }}</button>
    <button class="cc-btn sm" type="button" :disabled="!!sent" @click="choose('no')"><Spinner v-if="sent === 'no'" :size="11" />{{ t("chatInput.perm.denyNow") }}</button>
  </div>

  <template v-else>
    <p v-if="request.title" class="cc-lead selectable">{{ request.title }}</p>

    <template v-if="tool === 'Bash'">
      <pre class="cc-code selectable">{{ field(input, "command") }}</pre>
      <p v-if="bashNote" class="cc-note selectable">{{ bashNote }}</p>
    </template>

    <template v-else-if="tool === 'Edit' || tool === 'Write'">
      <div class="cc-file">
        <Icon name="file" :size="13" />
        <span class="cc-path" :title="file">{{ file }}</span>
        <span v-if="input.replace_all === true" class="cc-tag">{{ t("chatInput.perm.replaceAll") }}</span>
        <button class="cc-link" type="button" :disabled="!!sent" @click="startEdit">{{ editing ? t("chatInput.perm.showDiff") : t("chatInput.perm.editFirst") }}</button>
      </div>
      <textarea v-if="editing" v-model="edited" class="cc-textarea mono" spellcheck="false" :aria-label="t('chatInput.perm.editFirst')"></textarea>
      <div v-if="!editing || tool === 'Edit'" class="cc-diff selectable">
        <div v-for="(row, i) in diff" :key="i" :class="['row', row.kind]">
          <span class="mk">{{ row.kind === "add" ? "+" : row.kind === "del" ? "−" : "" }}</span><span>{{ row.text || " " }}</span>
        </div>
      </div>
    </template>

    <template v-else-if="tool === 'MultiEdit'">
      <div class="cc-file">
        <Icon name="file" :size="13" />
        <span class="cc-path" :title="file">{{ file }}</span>
        <span class="cc-tag">{{ t("chatInput.perm.edits", { count: edits.length }) }}</span>
      </div>
      <div class="cc-diff selectable">
        <template v-for="(each, n) in edits" :key="n">
          <div v-if="n > 0" class="row ctx"><span class="mk">⋯</span><span> </span></div>
          <div v-for="(row, i) in lineDiff(each.old, each.new)" :key="`${n}-${i}`" :class="['row', row.kind]">
            <span class="mk">{{ row.kind === "add" ? "+" : row.kind === "del" ? "−" : "" }}</span><span>{{ row.text || " " }}</span>
          </div>
        </template>
      </div>
    </template>

    <template v-else-if="tool === 'NotebookEdit'">
      <div class="cc-file">
        <Icon name="file" :size="13" />
        <span class="cc-path" :title="file">{{ file }}</span>
        <span v-if="field(input, 'cell_id')" class="cc-tag">{{ field(input, "cell_id") }}</span>
        <span v-if="field(input, 'edit_mode')" class="cc-tag">{{ field(input, "edit_mode") }}</span>
      </div>
      <pre v-if="field(input, 'new_source')" class="cc-code small selectable">{{ field(input, "new_source") }}</pre>
    </template>

    <template v-else-if="tool === 'WebFetch'">
      <button class="cc-file cc-url" type="button" :title="t('chatInput.perm.openAddress')" @click="openAddress">
        <Icon name="external" :size="13" />
        <span class="cc-path">{{ field(input, "url") }}</span>
      </button>
      <p v-if="field(input, 'prompt')" class="cc-note selectable">{{ field(input, "prompt") }}</p>
    </template>

    <template v-else-if="tool === 'WebSearch'">
      <pre class="cc-code selectable">{{ field(input, "query") }}</pre>
    </template>

    <template v-else>
      <div v-if="mcp" class="cc-tags">
        <span class="cc-label">{{ t("chatInput.perm.server") }}</span>
        <span class="cc-tag">{{ request.mcp_server?.name ?? mcp.server }}</span>
        <span class="cc-label">{{ t("chatInput.perm.tool") }}</span>
        <span class="cc-tag">{{ mcp.tool }}</span>
      </div>
      <div v-else-if="headline" class="cc-file">
        <Icon name="tools" :size="13" />
        <span class="cc-path" :title="headline">{{ headline }}</span>
      </div>
      <div>
        <button class="cc-link" type="button" :aria-expanded="showInput" @click="showInput = !showInput">
          <Icon name="chevron" :size="12" :class="['cc-tw', { open: showInput }]" />{{ t("chatInput.perm.input") }}
        </button>
      </div>
      <pre v-if="showInput" class="cc-code small selectable">{{ inputJson }}</pre>
    </template>

    <p v-if="description" class="cc-note selectable">{{ description }}</p>
    <p v-if="request.decision_reason" class="cc-note selectable">{{ t("chatInput.perm.reason", { reason: request.decision_reason }) }}</p>
    <p v-if="request.blocked_path" class="cc-note selectable">{{ t("chatInput.perm.blocked", { path: request.blocked_path }) }}</p>

    <div class="cc-actions" role="group">
      <button :class="['cc-btn', 'primary', { on: active === 'yes' }]" type="button" :disabled="!!sent" @click="choose('yes')">
        <Spinner v-if="sent === 'yes'" :size="11" /><span v-else class="cc-k">1</span>{{ editing && edited !== original ? t("chatInput.perm.yesEdited") : t("chatInput.perm.yes") }}
      </button>
      <button v-if="offer" :class="['cc-btn', { on: active === 'always' }]" type="button" :disabled="!!sent" @click="choose('always')">
        <Spinner v-if="sent === 'always'" :size="11" /><span v-else class="cc-k">2</span>{{ alwaysLabel }}
      </button>
      <button :class="['cc-btn', { on: active === 'no' }]" type="button" :disabled="!!sent" @click="choose('no')">
        <Spinner v-if="sent === 'no'" :size="11" /><span v-else class="cc-k">{{ choices.length }}</span>{{ t("chatInput.perm.deny") }}
      </button>
      <span class="cc-gap"></span>
      <button class="cc-btn quiet" type="button" :disabled="!!sent" :title="t('chatInput.perm.stopTitle')" @click="stop">
        <Spinner v-if="sent === 'stop'" :size="11" /><span v-else class="cc-k">esc</span>{{ t("chatInput.perm.stop") }}
      </button>
    </div>

    <div v-if="offer && (active === 'always' || offer.places.length > 1)" class="cc-always">
      <div v-if="offer.rules.length || offer.directories.length" class="cc-tags">
        <span class="cc-label">{{ t("chatInput.perm.allows") }}</span>
        <span v-for="rule in offer.rules" :key="rule" class="cc-tag" :title="rule">{{ rule }}</span>
        <span v-for="dir in offer.directories" :key="dir" class="cc-tag" :title="dir">{{ relativePath(dir, path) }}</span>
      </div>
      <div v-if="offer.places.length > 1" class="cc-tags">
        <span class="cc-label">{{ t("chatInput.perm.place") }}</span>
        <div class="cc-seg" role="radiogroup" :aria-label="t('chatInput.perm.place')">
          <button v-for="each in offer.places" :key="each" type="button" role="radio" :aria-checked="place === each" :class="{ on: place === each }" :disabled="!!sent" @click="place = each">{{ t(PLACES[each]) }}</button>
        </div>
      </div>
      <p v-else class="cc-note">{{ t("chatInput.perm.sessionOnly") }}</p>
    </div>

    <div v-if="telling" class="cc-tell">
      <input ref="tellField" v-model="message" class="cc-input" type="text" :placeholder="t('chatInput.perm.tell')" :aria-label="t('chatInput.perm.tell')" :disabled="!!sent" />
      <button class="cc-btn" type="button" :disabled="!!sent" @click="sendNo"><Spinner v-if="sent === 'no'" :size="11" />{{ t("chatInput.perm.denySend") }}</button>
    </div>
  </template>
</template>

<style scoped>
.compact {
  flex-wrap: nowrap;
}

.cc-url {
  width: 100%;
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  color: var(--text-nav);
  text-align: left;
}

.cc-url:hover {
  border-color: var(--line-lit);
}

.cc-always {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.cc-tw {
  margin-right: 4px;
  transition: transform 0.12s;
}

.cc-tw.open {
  transform: rotate(90deg);
}

.cc-link {
  display: inline-flex;
  align-items: center;
}
</style>
