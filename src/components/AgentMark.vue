<script setup lang="ts">
// An agent's mark (`<AgentMark :agent :size>`): Claude's exactly as ClaudeLogo draws it everywhere
// today, Codex's the OpenAI mark. `color` goes to either; without it each keeps its own.
import ClaudeLogo from "./ClaudeLogo.vue";
import OpenAILogo from "./OpenAILogo.vue";
import type { Agent } from "../lib/types";

defineProps<{ agent: Agent; size?: number; color?: string }>();
</script>

<template>
  <OpenAILogo v-if="agent === 'codex'" :size="size ?? 16" v-bind="color ? { color } : {}" />
  <ClaudeLogo v-else :size="size ?? 16" v-bind="color ? { color } : {}" />
</template>
