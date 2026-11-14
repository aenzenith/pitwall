<script setup lang="ts">
import { computed, reactive, ref } from "vue";

import Icon from "../../components/Icon.vue";
import Spinner from "../../components/Spinner.vue";
import type { ChatAnswer, ElicitationRequest } from "../../lib/chat/wire";
import { t } from "../../lib/i18n";
import { api } from "../../lib/store";

/**
 * An MCP server asking the user (elicitation; B8, V155): its message and either a form made from
 * its `requested_schema` (text, number, yes/no, one of a list; required ones marked) or, in `url`
 * mode, an address the user opens in the browser, by a click only. Accept, decline or cancel.
 */
const props = defineProps<{ request: ElicitationRequest }>();
const emit = defineEmits<{ answer: [answer: ChatAnswer]; refocus: [] }>();

type Field = { name: string; label: string; description: string; kind: "text" | "number" | "integer" | "boolean" | "enum"; options: string[]; required: boolean };

const isUrl = computed(() => props.request.mode === "url");

const fields = computed<Field[]>(() => {
  const schema = props.request.requested_schema;
  const required = new Set(Array.isArray(schema?.required) ? schema.required : []);
  return Object.entries(schema?.properties ?? {}).map(([name, spec]) => {
    const options = Array.isArray(spec?.enum) ? spec.enum.map(String) : [];
    const kind: Field["kind"] = options.length ? "enum" : spec?.type === "boolean" ? "boolean" : spec?.type === "number" ? "number" : spec?.type === "integer" ? "integer" : "text";
    return { name, label: spec?.title || name, description: spec?.description ?? "", kind, options, required: required.has(name) };
  });
});

// A number field's model comes back as a number (Vue's v-model on type=number).
const texts = reactive<Record<string, string | number>>({});
const flags = reactive<Record<string, boolean>>({});
for (const each of fields.value) {
  const given = props.request.requested_schema?.properties?.[each.name]?.default;
  if (each.kind === "boolean") flags[each.name] = given === true;
  else texts[each.name] = given === undefined || given === null ? "" : String(given);
}

function problem(each: Field): boolean {
  if (each.kind === "boolean") return false;
  const text = String(texts[each.name] ?? "").trim();
  if (!text) return each.required;
  if (each.kind === "number") return !Number.isFinite(Number(text));
  if (each.kind === "integer") return !Number.isInteger(Number(text));
  return false;
}

const valid = computed(() => fields.value.every((each) => !problem(each)));
const sent = ref<"accept" | "decline" | "cancel" | null>(null);

function content(): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const each of fields.value) {
    if (each.kind === "boolean") out[each.name] = flags[each.name] === true;
    else {
      const text = String(texts[each.name] ?? "").trim();
      if (!text) continue;
      out[each.name] = each.kind === "number" || each.kind === "integer" ? Number(text) : text;
    }
  }
  return out;
}

function send(action: "accept" | "decline" | "cancel"): void {
  if (sent.value || (action === "accept" && !isUrl.value && !valid.value)) return;
  sent.value = action;
  emit("answer", action === "accept" && !isUrl.value ? { kind: "elicitation", action, content: content() } : { kind: "elicitation", action });
}

function openAddress(): void {
  const url = props.request.url ?? "";
  if (/^https?:\/\//i.test(url)) void api.openUrl(url);
}

function key(event: KeyboardEvent): boolean {
  if (sent.value) return false;
  if (event.key === "Escape") {
    send("cancel");
    return true;
  }
  const target = event.target;
  if (event.key === "Enter" && !event.isComposing && target instanceof HTMLInputElement && target.type !== "checkbox") {
    send("accept");
    return true;
  }
  return false;
}

defineExpose({ key });
</script>

<template>
  <p v-if="request.title" class="cc-lead selectable">{{ request.title }}</p>
  <p class="cc-text selectable">{{ request.message }}</p>

  <template v-if="isUrl">
    <button class="address" type="button" :title="t('chatInput.elicit.open')" @click="openAddress">
      <Icon name="external" :size="13" />
      <span class="cc-path">{{ request.url }}</span>
    </button>
    <p class="cc-note">{{ t("chatInput.elicit.urlHint") }}</p>
  </template>

  <form v-else-if="fields.length" class="form" @submit.prevent="send('accept')">
    <div v-for="each in fields" :key="each.name" class="row">
      <label v-if="each.kind === 'boolean'" class="check">
        <input v-model="flags[each.name]" type="checkbox" role="switch" :disabled="!!sent" />
        <span class="name">{{ each.label }}</span>
      </label>
      <template v-else>
        <label class="name" :for="`el-${each.name}`">{{ each.label }}<span v-if="each.required" class="req" :title="t('chatInput.elicit.required')"> *</span></label>
        <select v-if="each.kind === 'enum'" :id="`el-${each.name}`" v-model="texts[each.name]" class="cc-select" :disabled="!!sent">
          <option v-if="!each.required" value="">{{ t("chatInput.elicit.none") }}</option>
          <option v-else value="" disabled>{{ t("chatInput.elicit.choose") }}</option>
          <option v-for="option in each.options" :key="option" :value="option">{{ option }}</option>
        </select>
        <input
          v-else
          :id="`el-${each.name}`"
          v-model="texts[each.name]"
          :class="['cc-input', { bad: problem(each) && !!String(texts[each.name] ?? '').trim() }]"
          :type="each.kind === 'text' ? 'text' : 'number'"
          :step="each.kind === 'integer' ? 1 : 'any'"
          :disabled="!!sent"
          spellcheck="false"
        />
      </template>
      <span v-if="each.description" class="cc-note">{{ each.description }}</span>
    </div>
  </form>

  <div class="cc-actions">
    <button class="cc-btn primary" type="button" :disabled="!!sent || (!isUrl && !valid)" @click="send('accept')">
      <Spinner v-if="sent === 'accept'" :size="11" />{{ isUrl ? t("chatInput.elicit.done") : t("chatInput.elicit.accept") }}
    </button>
    <button class="cc-btn" type="button" :disabled="!!sent" @click="send('decline')"><Spinner v-if="sent === 'decline'" :size="11" />{{ t("chatInput.elicit.decline") }}</button>
    <span class="cc-gap"></span>
    <button class="cc-btn quiet" type="button" :disabled="!!sent" @click="send('cancel')">
      <Spinner v-if="sent === 'cancel'" :size="11" /><span v-else class="cc-k">esc</span>{{ t("chatInput.elicit.cancel") }}
    </button>
  </div>
</template>

<style scoped>
.address {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  height: 28px;
  padding: 0 9px;
  border: 1px solid var(--line-strong);
  border-radius: var(--radius-control);
  background: var(--bg-input);
  text-align: left;
  color: var(--text-nav);
}

.address:hover {
  border-color: var(--line-lit);
}

.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
}

.row {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.name {
  font-size: 12.5px;
  font-weight: 500;
  color: var(--text-strong);
}

.req {
  color: var(--claude-text);
}

.check {
  display: flex;
  align-items: center;
  gap: 8px;
}

.cc-input.bad {
  border-color: var(--crash-line);
}
</style>
