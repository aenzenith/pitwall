<script setup lang="ts">
// Inline stroke icons; currentColor follows the button. Claude's own is components/ClaudeLogo.
withDefaults(
  defineProps<{
    name: "play" | "stop" | "restart" | "back" | "search" | "external" | "window" | "power" | "plus" | "star" | "star-filled" | "chevron" | "settings" | "editor" | "folder" | "close" | "pencil" | "terminal" | "coffee" | "calendar" | "grid" | "pulse" | "chat" | "link" | "copy" | "check" | "server" | "speaker" | "info" | "fuel" | "sparkles" | "lock" | "tools" | "wifi-off" |"board" | "list" | "chevron-down" | "flag" | "shuffle" | "clock" | "eye";
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
    <path v-else-if="name === 'chevron-down'" d="m6 9 6 6 6-6" />
    <path v-else-if="name === 'back'" d="m15 6-6 6 6 6" />
    <g v-else-if="name === 'search'"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></g>
    <g v-else-if="name === 'settings'"><path d="M4 7h10M18 7h2M4 17h4M12 17h8" /><circle cx="16" cy="7" r="2" /><circle cx="10" cy="17" r="2" /></g>
    <g v-else-if="name === 'editor'"><path d="m8 8-4 4 4 4" /><path d="m16 8 4 4-4 4" /><path d="m13.5 5-3 14" /></g>
    <g v-else-if="name === 'terminal'"><path d="m5 7 5 5-5 5" /><path d="M13 18h6" /></g>
    <path v-else-if="name === 'close'" d="M6.5 6.5l11 11M17.5 6.5l-11 11" />
    <g v-else-if="name === 'pencil'"><path d="M4.5 19.5h4l10-10a2.8 2.8 0 0 0-4-4l-10 10z" /><path d="m13.5 6.5 4 4" /></g>
    <g v-else-if="name === 'coffee'"><path d="M4.5 9.5h11V14a5 5 0 0 1-5 5h-1a5 5 0 0 1-5-5z" /><path d="M15.5 11h1.5a2.5 2.5 0 0 1 0 5h-1.8" /><path d="M8.5 3.5c-.7.9-.7 1.8 0 2.7M12 3.5c-.7.9-.7 1.8 0 2.7" /></g>
    <g v-else-if="name === 'link'"><path d="M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1" /><path d="M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1" /></g>
    <g v-else-if="name === 'copy'"><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></g>
    <path v-else-if="name === 'check'" d="m5 12.5 4.5 4.5L19 7.5" />
    <g v-else-if="name === 'server'"><rect x="4" y="4" width="16" height="7" rx="2" /><rect x="4" y="13" width="16" height="7" rx="2" /><path d="M8 7.5h.01M8 16.5h.01" /></g>
    <g v-else-if="name === 'speaker'"><path d="M11 5 6.5 9H3.5v6h3l4.5 4z" /><path d="M15.5 8.5a5 5 0 0 1 0 7" /><path d="M18.5 5.5a9 9 0 0 1 0 13" /></g>
    <g v-else-if="name === 'info'"><circle cx="12" cy="12" r="8.5" /><path d="M12 11v5M12 8h.01" /></g>
    <g v-else-if="name === 'calendar'"><rect x="4" y="5" width="16" height="15" rx="2.5" /><path d="M4 10h16M9 3v4M15 3v4" /></g>
    <g v-else-if="name === 'grid'"><rect x="4" y="4" width="6.5" height="6.5" rx="1.5" /><rect x="13.5" y="4" width="6.5" height="6.5" rx="1.5" /><rect x="4" y="13.5" width="6.5" height="6.5" rx="1.5" /><rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.5" /></g>
    <path v-else-if="name === 'pulse'" d="M3 12h4l2.5-6 5 12 2.5-6h4" />
    <g v-else-if="name === 'chat'"><path d="M5.5 5h13a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H11l-4.5 3.5V17h-1a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2z" /><path d="M8.5 11h.01M12 11h.01M15.5 11h.01" /></g>
    <g v-else-if="name === 'fuel'"><path d="M4.5 20V5.5a2 2 0 0 1 2-2h5.5a2 2 0 0 1 2 2V20" /><path d="M3 20h12.5M4.5 10h9.5" /><path d="M14 8.5h1.5a2 2 0 0 1 2 2v5.5a1.5 1.5 0 0 0 3 0V8.5L18.5 6" /></g>
    <g v-else-if="name === 'sparkles'"><path d="M10 4.5c.6 3.9 2.6 5.9 6.5 6.5-3.9.6-5.9 2.6-6.5 6.5-.6-3.9-2.6-5.9-6.5-6.5 3.9-.6 5.9-2.6 6.5-6.5z" /><path d="M18.5 3v4.5M16.25 5.25h4.5" /><path d="M18.5 16.5v3.5M16.75 18.25h3.5" /></g>
    <g v-else-if="name === 'board'"><rect x="4" y="4" width="4.5" height="15" rx="1.2" /><rect x="9.75" y="4" width="4.5" height="9.5" rx="1.2" /><rect x="15.5" y="4" width="4.5" height="12" rx="1.2" /></g>
    <path v-else-if="name === 'list'" d="M9 6.5h11M9 12h11M9 17.5h11M4.5 6.5h.01M4.5 12h.01M4.5 17.5h.01" />
    <g v-else-if="name === 'tools'" transform="translate(-.5 .5)"><path transform="rotate(45 12 12)" d="M13.6.99A4.6 4.6 0 0 1 13.8 9.53V21.5a1.8 1.8 0 0 1-3.6 0V9.53A4.6 4.6 0 0 1 10.4.99V5h3.2z" /><g transform="rotate(-45 12 12)"><path d="M12 1.4v8.8M12 13.8v2.8" /><rect x="10.3" y="16.6" width="3.4" height="6.8" rx="1.7" /></g></g>
    <g v-else-if="name === 'wifi-off'"><path d="M3.5 3.5l17 17" /><path d="M8.6 16.2a5 5 0 0 1 6.8 0" /><path d="M5 12.6a10 10 0 0 1 5-2.5M14.8 10.3a10 10 0 0 1 4.2 2.3" /><path d="M12 19.5h.01" /></g>
    <g v-else-if="name === 'lock'"><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></g>
    <g v-else-if="name === 'flag'"><path d="M5 21V4" /><path d="M5 4.6c3.4-1.7 6.2 1.7 9.6.2 1.5-.7 2.9-1 4.4-.7v8.4c-1.5-.3-2.9 0-4.4.7-3.4 1.5-6.2-1.9-9.6-.2" /></g>
    <g v-else-if="name === 'shuffle'"><path d="M4 7h3.5c5 0 5 10 10 10H20" /><path d="M4 17h3.5c1.6 0 2.7-1 3.6-2.4M14 9.4c.9-1.4 2-2.4 3.5-2.4H20" /><path d="m17.5 4.5 2.5 2.5-2.5 2.5M17.5 14.5l2.5 2.5-2.5 2.5" /></g>
    <g v-else-if="name === 'clock'"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></g>
    <g v-else-if="name === 'eye'"><path d="M2.5 12s3.5-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.5 6.5-9.5 6.5S2.5 12 2.5 12z" /><circle cx="12" cy="12" r="2.8" /></g>
    <path v-else-if="name === 'folder'" d="M3.5 7a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />
  </svg>
</template>
