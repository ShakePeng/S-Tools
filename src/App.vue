<script setup lang="ts">
import { computed } from "vue";
import CaptureView from "./modules/screenshot/CaptureView.vue";
import TranslateView from "./modules/translate/TranslateView.vue";
import OnboardingView from "./shell/OnboardingView.vue";
import ShellView from "./shell/ShellView.vue";

const params = new URLSearchParams(window.location.search);
const view = computed(() => params.get("view") || "main");
</script>

<template>
  <ShellView v-if="view === 'main'" />
  <TranslateView v-else-if="view === 'translate'" />
  <CaptureView v-else-if="view === 'capture'" />
  <OnboardingView v-else-if="view === 'onboarding'" />
  <ShellView v-else />
</template>
