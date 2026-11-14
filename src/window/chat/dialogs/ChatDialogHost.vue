<script setup lang="ts">
import { computed, type Component } from "vue";

import type { ChatDialogEmits, ChatDialogKind, ChatDialogProps } from "../../../lib/chat/ui";
import AgentsDialog from "./AgentsDialog.vue";
import BtwDialog from "./BtwDialog.vue";
import ContextDialog from "./ContextDialog.vue";
import ExportDialog from "./ExportDialog.vue";
import HelpDialog from "./HelpDialog.vue";
import HooksDialog from "./HooksDialog.vue";
import McpDialog from "./McpDialog.vue";
import MemoryDialog from "./MemoryDialog.vue";
import ModeDialog from "./ModeDialog.vue";
import ModelDialog from "./ModelDialog.vue";
import PermissionsDialog from "./PermissionsDialog.vue";
import PlanDialog from "./PlanDialog.vue";
import ResumeDialog from "./ResumeDialog.vue";
import RewindDialog from "./RewindDialog.vue";
import SkillsDialog from "./SkillsDialog.vue";
import StatusDialog from "./StatusDialog.vue";
import StylesDialog from "./StylesDialog.vue";
import UsageDialog from "./UsageDialog.vue";

/**
 * The chat's dialogs, one at a time: the one `open` names, given the chat it belongs to. Each asks
 * the chat's Claude Code what it shows (`chat_control`) and says so when this version doesn't
 * answer; what one passes back (text for the composer, a fork, a session to resume) goes on to
 * the chat as `ChatDialogEmits` says. Esc, a click outside or its own button closes it (`close`).
 */
const props = defineProps<ChatDialogProps>();
const emit = defineEmits<ChatDialogEmits>();

const DIALOGS: Record<ChatDialogKind, Component> = {
  model: ModelDialog,
  mode: ModeDialog,
  context: ContextDialog,
  usage: UsageDialog,
  status: StatusDialog,
  mcp: McpDialog,
  permissions: PermissionsDialog,
  hooks: HooksDialog,
  memory: MemoryDialog,
  skills: SkillsDialog,
  styles: StylesDialog,
  agents: AgentsDialog,
  export: ExportDialog,
  help: HelpDialog,
  rewind: RewindDialog,
  btw: BtwDialog,
  resume: ResumeDialog,
  plan: PlanDialog,
};

const shown = computed(() => (props.open ? (DIALOGS[props.open.kind] ?? null) : null));

/** What every dialog is given; the conversation's messages only to the one that rewinds. */
const given = computed(() => ({
  id: props.id,
  path: props.path,
  session: props.session,
  info: props.info,
  state: props.state,
  arg: props.open?.arg,
  ...(props.open?.kind === "rewind" ? { messages: props.messages } : {}),
}));
</script>

<template>
  <!-- Keyed by its kind and chat: another dialog, or the same for another chat, starts afresh. -->
  <component
    :is="shown"
    v-if="shown && open"
    :key="`${id}:${open.kind}`"
    v-bind="given"
    @close="emit('close')"
    @insert="(text: string) => emit('insert', text)"
    @fork="(at: string | null) => emit('fork', at)"
    @resume="(session: string) => emit('resume', session)"
  />
</template>
