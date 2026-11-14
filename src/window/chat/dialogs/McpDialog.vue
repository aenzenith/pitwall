<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";

import Icon from "../../../components/Icon.vue";
import Spinner from "../../../components/Spinner.vue";
import { t, type Key } from "../../../lib/i18n";
import { api } from "../../../lib/store";
import DialogFrame from "./DialogFrame.vue";
import NotAnswered from "./NotAnswered.vue";
import { errorText, projectName, send, useAsk, useListNav, type DialogProps } from "./shared";

/**
 * MCP servers (B8): each server of the session (`mcp_status`) with its state, its tools and where
 * it is set; a row opens to what can be done with it: reconnect (`mcp_reconnect`), turn it on or
 * off for this session (`mcp_toggle`), sign in where it asks for that (`mcp_authenticate`, whose
 * address opens in the browser on a click only). Adding and removing servers is Claude Code's
 * own (its settings, `claude mcp`).
 */
const props = defineProps<DialogProps>();
const emit = defineEmits<{ close: [] }>();

type Server = {
  name: string;
  status?: string;
  serverInfo?: { name?: string; version?: string };
  error?: string;
  error_code?: string;
  transport?: string;
  scope?: string;
  tools?: unknown[] | number;
};
type Status = { mcpServers?: Server[] };
type Auth = { authUrl?: string; requiresUserAction?: boolean };

const frame = ref<InstanceType<typeof DialogFrame> | null>(null);
const listEl = ref<HTMLElement | null>(null);
const status = useAsk<Status>(() => props.id);
onMounted(() => void status.load({ kind: "mcpStatus" }));

const servers = computed(() => (status.value.value?.mcpServers ?? []).filter((server) => typeof server.name === "string"));

const STATES: Record<string, Key> = {
  connected: "chatDialog.mcp.connected",
  failed: "chatDialog.mcp.failed",
  "needs-auth": "chatDialog.mcp.needsAuth",
  pending: "chatDialog.mcp.pending",
  disabled: "chatDialog.mcp.disabled",
};
const SCOPES: Record<string, Key> = {
  user: "chatDialog.mcp.scope.user",
  project: "chatDialog.mcp.scope.project",
  local: "chatDialog.mcp.scope.local",
  enterprise: "chatDialog.mcp.scope.managed",
  managed: "chatDialog.mcp.scope.managed",
  dynamic: "chatDialog.mcp.scope.session",
  plugin: "chatDialog.mcp.scope.plugin",
};

function toolCount(server: Server): number | null {
  if (Array.isArray(server.tools)) return server.tools.length;
  return typeof server.tools === "number" ? server.tools : null;
}

function meta(server: Server): string {
  const parts: string[] = [];
  if (server.scope) parts.push(SCOPES[server.scope] ? t(SCOPES[server.scope]) : server.scope === "claudeai" ? "claude.ai" : server.scope);
  const tools = toolCount(server);
  if (server.status === "connected" && tools !== null) parts.push(t("chatDialog.mcp.tools", { count: tools }));
  return parts.join(" · ");
}

/** Sign-in is offered where the server asks for it, or its sign-in was turned down. */
const canSignIn = (server: Server) => server.status === "needs-auth" || server.error_code === "CLAUDEAI_BEARER_REJECTED" || server.error_code === "CLIENT_REGISTRATION_UNAVAILABLE";

/** The server whose row is open. */
const opened = ref<string | null>(null);
/** `<name>:reconnect`, `<name>:toggle`, `<name>:auth`. */
const busy = ref<string | null>(null);
const actionError = ref<string | null>(null);
/** The sign-in addresses the CLI gave, by server, for a click to open. */
const addresses = reactive<Record<string, string>>({});

function toggleOpen(server: Server): void {
  opened.value = opened.value === server.name ? null : server.name;
}

const nav = useListNav(
  () => servers.value.length,
  (index) => toggleOpen(servers.value[index]),
  listEl,
);

async function refresh(): Promise<void> {
  await status.load({ kind: "mcpStatus" });
}

async function reconnect(server: Server): Promise<void> {
  if (busy.value) return;
  busy.value = `${server.name}:reconnect`;
  actionError.value = await send(props.id, { kind: "mcpReconnect", server: server.name });
  busy.value = null;
  await refresh();
}

async function toggle(server: Server, event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  const enabled = box.checked;
  box.checked = server.status !== "disabled";
  if (busy.value) return;
  busy.value = `${server.name}:toggle`;
  actionError.value = await send(props.id, { kind: "mcpToggle", server: server.name, enabled });
  busy.value = null;
  await refresh();
}

async function signIn(server: Server): Promise<void> {
  if (busy.value) return;
  busy.value = `${server.name}:auth`;
  actionError.value = null;
  try {
    const auth = await api.chatControl<Auth | null>(props.id, { kind: "mcpAuthenticate", server: server.name });
    if (auth?.authUrl && /^https?:\/\//i.test(auth.authUrl)) addresses[server.name] = auth.authUrl;
    else await refresh();
  } catch (reason) {
    actionError.value = errorText(reason);
  } finally {
    busy.value = null;
  }
}

function host(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const rowId = (index: number) => `chat-mcp-${props.id}-${index}`;
</script>

<template>
  <DialogFrame ref="frame" :title="t('chatDialog.mcp.title')" :sub="projectName(path)" :width="500" @close="emit('close')" @key="nav.onKey">
    <template #tools>
      <button type="button" class="icon-button" :aria-label="t('chatDialog.refresh')" :title="t('chatDialog.refresh')" :disabled="status.busy.value" @click="refresh">
        <Spinner v-if="status.busy.value && status.value.value" :size="12" />
        <Icon v-else name="restart" :size="14" />
      </button>
    </template>

    <div v-if="status.busy.value && !status.value.value" class="state"><Spinner :size="14" /></div>
    <NotAnswered v-else-if="status.error.value && !status.value.value" :id="id" :state="state" :error="status.error.value" @done="frame?.close()" />
    <p v-else-if="!servers.length" class="state">{{ t("chatDialog.mcp.empty") }}</p>

    <div v-else ref="listEl" class="scroll list" role="listbox" tabindex="0" data-focus :aria-label="t('chatDialog.mcp.title')" :aria-activedescendant="rowId(nav.active.value)">
      <template v-for="(server, i) in servers" :key="server.name">
        <div
          :id="rowId(i)"
          :data-row="i"
          role="option"
          :aria-selected="i === nav.active.value"
          :aria-expanded="opened === server.name"
          :class="['row', { active: i === nav.active.value }]"
          @click="
            nav.active.value = i;
            toggleOpen(server);
          "
        >
          <span class="mark">
            <Spinner v-if="server.status === 'pending' || busy?.startsWith(`${server.name}:`)" :size="11" />
            <span v-else :class="['dot', server.status]"></span>
          </span>
          <span class="text">
            <span class="name">{{ server.name }}</span>
            <span v-if="meta(server)" class="desc one">{{ meta(server) }}</span>
          </span>
          <span v-if="server.status !== 'connected' && server.status && STATES[server.status]" :class="['said', server.status]">{{ t(STATES[server.status]) }}</span>
          <!-- On or off for this session; the click is the switch's alone. -->
          <input
            type="checkbox"
            role="switch"
            :aria-label="t('chatDialog.mcp.on', { name: server.name })"
            :checked="server.status !== 'disabled'"
            :disabled="!!busy"
            @click.stop
            @change="toggle(server, $event)"
          />
          <Icon name="chevron-down" :size="13" :class="['chevron', { open: opened === server.name }]" />
        </div>

        <div v-if="opened === server.name" class="details" @click.stop>
          <p v-if="server.error" class="error">{{ server.error }}</p>
          <p v-if="server.transport || server.serverInfo?.version" class="facts">
            {{ [server.transport, server.serverInfo?.name && server.serverInfo?.version ? `${server.serverInfo.name} ${server.serverInfo.version}` : ""].filter(Boolean).join(" · ") }}
          </p>
          <div class="buttons">
            <button v-if="server.status !== 'disabled'" type="button" class="control small" :disabled="!!busy" :aria-busy="busy === `${server.name}:reconnect`" @click="reconnect(server)">
              <span :class="{ hidden: busy === `${server.name}:reconnect` }">{{ t("chatDialog.mcp.reconnect") }}</span>
              <Spinner v-if="busy === `${server.name}:reconnect`" class="over" :size="11" />
            </button>
            <button v-if="canSignIn(server)" type="button" class="control small" :disabled="!!busy" :aria-busy="busy === `${server.name}:auth`" @click="signIn(server)">
              <span :class="{ hidden: busy === `${server.name}:auth` }">{{ t("chatDialog.mcp.signIn") }}</span>
              <Spinner v-if="busy === `${server.name}:auth`" class="over" :size="11" />
            </button>
          </div>
          <div v-if="addresses[server.name]" class="auth">
            <button type="button" class="control small" @click="api.openUrl(addresses[server.name])">
              <Icon name="external" :size="12" />
              {{ t("chatDialog.mcp.openSignIn") }}
            </button>
            <span class="host mono">{{ host(addresses[server.name]) }}</span>
            <p class="note">{{ t("chatDialog.mcp.signInNote") }}</p>
          </div>
        </div>
      </template>
    </div>

    <NotAnswered v-if="actionError" :id="id" :state="state" :error="actionError" @done="frame?.close()" />

    <p class="note">{{ t("chatDialog.mcp.note") }}</p>

    <template #foot>
      <button type="button" class="control primary" @click="frame?.close()">{{ t("chatDialog.done") }}</button>
    </template>
  </DialogFrame>
</template>

<style scoped src="./dialog.css"></style>

<style scoped>
.list {
  max-height: 360px;
}

.dot {
  width: 7px;
  height: 7px;
  box-sizing: border-box;
  border-radius: 50%;
  background: var(--run);
}

.dot.failed {
  background: var(--crash);
}

.dot.needs-auth {
  background: var(--warn);
}

/* Off: the list's idle ring. */
.dot.disabled {
  border: 1.5px solid var(--idle-ring);
  background: none;
}

.said {
  flex-shrink: 0;
  font-size: 11.5px;
  color: var(--text-subtle);
  white-space: nowrap;
}

.said.failed {
  color: var(--crash-text);
}

.said.needs-auth {
  color: var(--warn-text);
}

.chevron {
  flex-shrink: 0;
  color: var(--text-faint);
  transition: transform 0.15s;
}

.chevron.open {
  transform: rotate(180deg);
}

.details {
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 2px 0 6px 34px;
  padding: 10px 12px;
  border-radius: 8px;
  background: var(--bg-well);
  box-shadow: inset 0 0 0 1px var(--line-soft);
}

.facts {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-subtle);
}

.buttons,
.auth {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.auth .note {
  flex-basis: 100%;
}

.host {
  font-size: 11.5px;
  color: var(--text-subtle);
}

.icon-button {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text-muted);
}

.icon-button:hover:not(:disabled) {
  background: var(--bg-icon-hover);
  color: var(--text);
}

@media (prefers-reduced-motion: reduce) {
  .chevron {
    transition: none;
  }
}
</style>
