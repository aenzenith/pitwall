<script setup lang="ts">
// The chat's own stroke icons, drawn as components/Icon draws its own (currentColor, 24-unit box):
// the ones the conversation and its column need that the app's set doesn't have.
withDefaults(
  defineProps<{
    name: "file" | "file-plus" | "terminal" | "pencil" | "search" | "globe" | "plug" | "spark" | "agent" | "list" | "question" | "plan" | "tool" | "column" | "bookmark" | "bookmark-on" | "fork" | "rewind" | "brain" | "compact" | "copy" | "edit" | "shield" | "zap" | "ban";
    size?: number;
  }>(),
  { size: 14 },
);
</script>

<template>
  <svg :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <g v-if="name === 'file'"><path d="M14 3.5H7.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V8z" /><path d="M14 3.5V8h4.5" /></g>
    <g v-else-if="name === 'file-plus'"><path d="M14 3.5H7.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V8z" /><path d="M14 3.5V8h4.5M12 11.5v5M9.5 14h5" /></g>
    <g v-else-if="name === 'terminal'"><path d="m5 7.5 4.5 4.5L5 16.5" /><path d="M12 17h7" /></g>
    <g v-else-if="name === 'pencil' || name === 'edit'"><path d="M15.5 5.5 18.5 8.5 9 18H6v-3z" /><path d="m13.5 7.5 3 3" /></g>
    <g v-else-if="name === 'search'"><circle cx="11" cy="11" r="6" /><path d="m19.5 19.5-4-4" /></g>
    <g v-else-if="name === 'globe'"><circle cx="12" cy="12" r="8" /><path d="M4 12h16M12 4c2.2 2.3 3.2 5 3.2 8s-1 5.7-3.2 8c-2.2-2.3-3.2-5-3.2-8s1-5.7 3.2-8z" /></g>
    <g v-else-if="name === 'plug'"><path d="M9 4v4M15 4v4" /><path d="M6.5 8h11v3a5.5 5.5 0 0 1-11 0z" /><path d="M12 16.5V20" /></g>
    <g v-else-if="name === 'spark'"><path d="M12 4.5 13.6 10l5.4 2-5.4 2-1.6 5.5-1.6-5.5L5 12l5.4-2z" /></g>
    <g v-else-if="name === 'agent'"><rect x="5" y="8" width="14" height="10" rx="3" /><path d="M12 4.5V8M9.5 12.5v1M14.5 12.5v1" /></g>
    <g v-else-if="name === 'list'"><path d="M10 7h9M10 12h9M10 17h9" /><path d="m4.5 7 1.2 1.2L7.8 6M4.5 12l1.2 1.2L7.8 11" /><circle cx="6" cy="17" r="1" /></g>
    <g v-else-if="name === 'question'"><circle cx="12" cy="12" r="8" /><path d="M9.8 9.6a2.3 2.3 0 1 1 3.2 2.1c-.6.3-1 .8-1 1.5v.4M12 16.6h.01" /></g>
    <g v-else-if="name === 'plan'"><rect x="5.5" y="4.5" width="13" height="15" rx="2" /><path d="M9 9h6M9 12.5h6M9 16h3.5" /></g>
    <g v-else-if="name === 'tool'"><path d="M14.5 5.5a4 4 0 0 0-5 5L4.5 15.5l4 4 5-5a4 4 0 0 0 5-5l-2.5 2.5-2.5-.5-.5-2.5z" /></g>
    <g v-else-if="name === 'column'"><rect x="3.5" y="5" width="17" height="14" rx="2.5" /><path d="M14 5v14" /></g>
    <path v-else-if="name === 'bookmark'" d="M7 4.5h10v15l-5-3.5-5 3.5z" />
    <path v-else-if="name === 'bookmark-on'" d="M7 4.5h10v15l-5-3.5-5 3.5z" fill="currentColor" />
    <g v-else-if="name === 'fork'"><circle cx="7" cy="5.5" r="1.8" /><circle cx="17" cy="5.5" r="1.8" /><circle cx="12" cy="18.5" r="1.8" /><path d="M7 7.3v1.2a3 3 0 0 0 3 3h4a3 3 0 0 0 3-3V7.3M12 11.5v5.2" /></g>
    <g v-else-if="name === 'rewind'"><path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3" /><path d="M4.5 4.5v4h4" /></g>
    <g v-else-if="name === 'brain'"><path d="M12 5.5C10.6 4 8 4.6 7.6 6.6 5.6 6.9 4.6 8.9 5.4 10.6 4 11.7 4.1 13.9 5.6 14.9 5.3 16.9 6.9 18.6 8.9 18.2 9.7 19.6 11.2 19.8 12 18.8z" /><path d="M12 5.5c1.4-1.5 4-.9 4.4 1.1 2 .3 3 2.3 2.2 4 1.4 1.1 1.3 3.3-.2 4.3.3 2-1.3 3.7-3.3 3.3-.8 1.4-2.3 1.6-3.1.6" /><path d="M7.6 6.6c.1 1 .6 1.8 1.5 2.2M5.4 10.6c.9.6 2.2.6 3-.2M5.6 14.9c1 0 1.9-.6 2.3-1.5M16.4 6.6c-.1 1-.6 1.8-1.5 2.2M18.6 10.6c-.9.6-2.2.6-3-.2M18.4 14.9c-1 0-1.9-.6-2.3-1.5" /></g>
    <g v-else-if="name === 'compact'"><path d="M8 4.5 12 8.5l4-4M8 19.5l4-4 4 4" /><path d="M5 12h14" /></g>
    <path v-else-if="name === 'shield'" d="M12 4 5.5 6.5V11c0 4.2 2.7 7.2 6.5 8.8 3.8-1.6 6.5-4.6 6.5-8.8V6.5z" />
    <path v-else-if="name === 'zap'" d="M13 3.5 5.5 13.5H12l-1 7 7.5-10H12z" />
    <g v-else-if="name === 'ban'"><circle cx="12" cy="12" r="7.5" /><path d="m6.7 6.7 10.6 10.6" /></g>
    <g v-else-if="name === 'copy'"><rect x="8.5" y="8.5" width="11" height="11" rx="2" /><path d="M15.5 8.5V6.5a2 2 0 0 0-2-2h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h2" /></g>
  </svg>
</template>
