<script setup lang="ts">
import { computed } from "vue";

import Icon from "../../components/Icon.vue";
import PitwallGlyph from "../../components/PitwallGlyph.vue";
import { originView } from "../../lib/sessions";
import { snapshot } from "../../lib/store";
import type { SessionOrigin } from "../../lib/types";

/** Where a session runs, as an icon and a name; `long` names it in full (the details), else the
 * row's short name with the full one as its tooltip. */
const props = defineProps<{ origin: SessionOrigin; project: string; long?: boolean }>();

const view = computed(() => originView(props.origin, props.project, snapshot.value?.settings.editor));
</script>

<template>
  <span class="origin" :title="long ? undefined : view.long">
    <PitwallGlyph v-if="view.icon === 'pitwall'" :size="12" />
    <Icon v-else :name="view.icon" :size="12" />
    <span class="origin-text">{{ long ? view.long : view.short }}</span>
  </span>
</template>

<style scoped>
.origin {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}

.origin svg {
  flex-shrink: 0;
}

.origin-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
