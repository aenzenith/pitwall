<script setup lang="ts">
// Inline stroke icons; currentColor follows the button.
withDefaults(
  defineProps<{
    name: "play" | "stop" | "restart" | "back" | "search" | "external" | "window" | "power" | "plus" | "star" | "star-filled" | "chevron" | "settings" | "editor" | "folder" | "close" | "pencil" | "terminal" | "coffee";
    size?: number;
  }>(),
  { size: 15 },
);
</script>

<template>
  <svg :width="size" :height="size" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path v-if="name === 'play'" d="M8 5.5v13l10.5-6.5z" />
    <rect v-else-if="name === 'stop'" x="6" y="6" width="12" height="12" rx="2" />
    <g v-else-if="name === 'restart'"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></g>
    <path v-else-if="name === 'external'" d="M14 4h6v6M20 4l-9 9M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4" />
    <g v-else-if="name === 'window'"><rect x="3" y="4.5" width="18" height="15" rx="2.5" /><path d="M3 9h18" /></g>
    <g v-else-if="name === 'power'"><path d="M12 3v8" /><path d="M6.3 6.8a8 8 0 1 0 11.4 0" /></g>
    <path v-else-if="name === 'plus'" d="M12 5v14M5 12h14" />
    <path v-else-if="name === 'star'" d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" />
    <path v-else-if="name === 'star-filled'" d="m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" fill="currentColor" />
    <path v-else-if="name === 'chevron'" d="m9 6 6 6-6 6" />
    <path v-else-if="name === 'back'" d="m15 6-6 6 6 6" />
    <g v-else-if="name === 'search'"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></g>
    <g v-else-if="name === 'settings'"><path d="M4 7h10M18 7h2M4 17h4M12 17h8" /><circle cx="16" cy="7" r="2" /><circle cx="10" cy="17" r="2" /></g>
    <g v-else-if="name === 'editor'"><path d="m8 8-4 4 4 4" /><path d="m16 8 4 4-4 4" /><path d="m13.5 5-3 14" /></g>
    <g v-else-if="name === 'terminal'"><path d="m5 7 5 5-5 5" /><path d="M13 18h6" /></g>
    <path v-else-if="name === 'close'" d="M6.5 6.5l11 11M17.5 6.5l-11 11" />
    <g v-else-if="name === 'pencil'"><path d="M4.5 19.5h4l10-10a2.8 2.8 0 0 0-4-4l-10 10z" /><path d="m13.5 6.5 4 4" /></g>
    <g v-else-if="name === 'coffee'"><path d="M4.5 9.5h11V14a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5z" /><path d="M15.5 11h1.5a2.5 2.5 0 0 1 0 5h-1.8" /><path d="M8.5 3.5c-.7.9-.7 1.8 0 2.7M12 3.5c-.7.9-.7 1.8 0 2.7" /></g>
    <path v-else-if="name === 'folder'" d="M3.5 7a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
  </svg>
</template>
