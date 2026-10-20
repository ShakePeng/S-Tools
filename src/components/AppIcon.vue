<script setup lang="ts">
withDefaults(defineProps<{ size?: number; decorative?: boolean }>(), {
  size: 40,
  decorative: true,
});
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 64 64"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    :aria-hidden="decorative"
  >
    <rect width="64" height="64" rx="15" fill="#17181B" />
    <path
      d="M43.4 16.8C39.9 14.9 35.7 14 31.5 14C22.8 14 16 19.4 16 26.8C16 34.6 22.7 37.2 30.2 39.1C36.4 40.7 39.7 41.9 39.7 45.3C39.7 48.6 36.3 50.4 32.1 50.4C27 50.4 22.4 48.7 18.9 46.2L16.6 49.7C20.5 52.7 25.9 54.3 32 54.3C41.2 54.3 48 49.5 48 42.1C48 34.8 42.2 32.1 34.5 30.2C28 28.6 24.3 27.2 24.3 24.1C24.3 21 27.4 19 31.7 19C35.2 19 38.7 19.8 41.5 21.3L43.4 16.8Z"
      fill="#F7F4EE"
    />
    <path
      d="M32 54.3C41.2 54.3 48 49.5 48 42.1C48 37.4 45.2 34.7 40.8 33"
      stroke="url(#paint0_linear)"
      stroke-width="6"
      stroke-linecap="round"
    />
    <defs>
      <linearGradient id="paint0_linear" x1="33" y1="53" x2="48" y2="35" gradientUnits="userSpaceOnUse">
        <stop stop-color="#F7931E" />
        <stop offset="1" stop-color="#FFB340" />
      </linearGradient>
    </defs>
  </svg>
</template>
