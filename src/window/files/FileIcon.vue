<script setup lang="ts">
// A file's icon as VS Code draws it with its default theme, Seti (lib/fileIcons.ts): a character
// of seti.woff in the icon's own colour, at VS Code's size (a 16 px box, the character at 150 % of
// its 13 px text). Git's colour stays on the file's name; the icon keeps its own. A folder keeps
// the app's glyph: Seti has none.
import { computed } from "vue";

import { fileIcon } from "../../lib/fileIcons";
import { resolvedTheme } from "../../lib/theme";

const props = withDefaults(defineProps<{ path: string; size?: number }>(), { size: 16 });
const icon = computed(() => fileIcon(props.path, resolvedTheme.value));
</script>

<template>
  <span class="file-icon" :style="{ width: `${size}px`, height: `${size}px`, fontSize: `${(size * 19.5) / 16}px`, color: icon.colour ?? undefined }" aria-hidden="true">{{ icon.glyph }}</span>
</template>

<style>
/*! seti.woff: Seti UI, Copyright (c) 2014 Jesse Weed, MIT licence; from Visual Studio Code's
   theme-seti, Copyright (c) Microsoft Corporation, MIT licence (src/assets/seti/LICENSE). */
@font-face {
  font-family: "Seti Icons";
  src: url("../../assets/seti/seti.woff") format("woff");
  font-weight: normal;
  font-style: normal;
  font-display: block;
}
</style>

<style scoped>
/* VS Code's place for it: the character from the box's left edge (its ink sits a little in from
   there and ends at the box's right edge), centred on the row. */
.file-icon {
  display: inline-flex;
  align-items: center;
  justify-content: flex-start;
  flex-shrink: 0;
  overflow: visible;
  font-family: "Seti Icons";
  font-style: normal;
  font-weight: normal;
  line-height: 1;
  -webkit-font-smoothing: antialiased;
  user-select: none;
}
</style>
