<script setup lang="ts">
defineProps<{ name: string; size?: number }>();
</script>

<template>
  <svg
    :width="size || 18"
    :height="size || 18"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path v-if="name === 'home'" d="M3 10.8 12 3l9 7.8v8.4a1.8 1.8 0 0 1-1.8 1.8H4.8A1.8 1.8 0 0 1 3 19.2z" />
    <path v-else-if="name === 'translate'" d="M4 5h9M8.5 3v2c0 4.2-2.1 7.4-5.5 9M5 9c1.2 2.7 3.4 4.6 6.3 5.7M14 19l3.2-9 3.2 9M15.2 16h4.1" />
    <path v-else-if="name === 'screenshot'" d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M8 21H5a2 2 0 0 1-2-2v-3M16 21h3a2 2 0 0 0 2-2v-3M8 8h8v8H8z" />
    <path v-else-if="name === 'history'" d="M3 12a9 9 0 1 0 3-6.7L3 8M3 3v5h5M12 7v5l3 2" />
    <g v-else-if="name === 'settings'">
      <path d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Z" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6 1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" />
    </g>
    <path v-else-if="name === 'update'" d="M20 7v5h-5M4 17v-5h5M6.1 8.2A7 7 0 0 1 18.3 7M17.9 15.8A7 7 0 0 1 5.7 17" />
    <path v-else-if="name === 'copy'" d="M8 8h11v12H8zM5 16H3V3h12v2" />
    <path v-else-if="name === 'external'" d="M14 4h6v6M20 4l-9 9M19 13v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5" />
    <path v-else-if="name === 'check'" d="m5 12 4 4L19 6" />
    <path v-else-if="name === 'alert'" d="M12 3 2.8 20h18.4zM12 9v4M12 17h.01" />
    <path v-else-if="name === 'keyboard'" d="M3 6h18v12H3zM6 10h.01M9.5 10h.01M13 10h.01M16.5 10h.01M6 14h.01M18 14h.01M9 14h6" />
    <path v-else-if="name === 'shield'" d="M12 3 20 6v5c0 5-3.4 8.6-8 10-4.6-1.4-8-5-8-10V6zM9 12l2 2 4-5" />
    <path v-else-if="name === 'spark'" d="m12 3 1.5 5.5L19 10l-5.5 1.5L12 17l-1.5-5.5L5 10l5.5-1.5zM19 16l.7 2.3L22 19l-2.3.7L19 22l-.7-2.3L16 19l2.3-.7z" />
    <path v-else-if="name === 'provider'" d="M4 7h16M7 3v4M17 3v4M5 11h14v9H5zM8 15h3M8 18h6" />
    <path v-else-if="name === 'trash'" d="M4 7h16M9 7V4h6v3M7 7l1 13h8l1-13M10 11v5M14 11v5" />
    <path v-else-if="name === 'edit'" d="m4 20 4.5-1 10-10-3.5-3.5-10 10zM13.8 6.8l3.5 3.5" />
    <path v-else-if="name === 'image'" d="M4 5h16v14H4zM7 15l3-3 3 3 2-2 3 3M8 9h.01" />
    <path v-else-if="name === 'close'" d="m6 6 12 12M18 6 6 18" />
    <path v-else-if="name === 'arrow'" d="M5 12h14M14 7l5 5-5 5" />
    <path v-else-if="name === 'pin'" d="m9 3 6 6-2 2 4 6-4 4-6-4-2 2-6-6 2-2-2-6z" />
    <path v-else-if="name === 'save'" d="M5 3h12l3 3v15H4V3zM8 3v6h8V3M8 21v-7h8v7" />
    <path v-else-if="name === 'plus'" d="M12 5v14M5 12h14" />
    <path v-else-if="name === 'language'" d="M4 5h10M9 3v2c0 5-2.4 8.7-6 10M6 9c1.4 3 3.7 5.1 7 6M14 19l3.2-9 3.2 9M15.2 16h4.1" />
  </svg>
</template>
