<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import {
  buildHotkeyFromEvent,
  formatHotkey,
  isModifierOnly,
} from "../lib/hotkeys";
import { call, isTauri } from "../lib/ipc";
import UiIcon from "./UiIcon.vue";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    label?: string;
    conflictValue?: string;
    disabled?: boolean;
  }>(),
  {
    label: "快捷键",
    disabled: false,
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const root = ref<HTMLElement | null>(null);
const button = ref<HTMLButtonElement | null>(null);
const recording = ref(false);
const preparing = ref(false);
const validating = ref(false);
const hint = ref("");
const isMac = computed(() => navigator.platform.toLowerCase().includes("mac"));
const displayValue = computed(() => formatHotkey(props.modelValue, isMac.value));
let captureGeneration = 0;

async function suspendShortcuts() {
  if (!isTauri()) return;
  await call("suspend_shortcuts");
}

async function resumeShortcuts() {
  if (!isTauri()) return;
  await call("resume_shortcuts");
}

function addCaptureListeners() {
  window.addEventListener("keydown", onKeydown, true);
  document.addEventListener("pointerdown", onOutsidePointerDown, true);
}

function removeCaptureListeners() {
  window.removeEventListener("keydown", onKeydown, true);
  document.removeEventListener("pointerdown", onOutsidePointerDown, true);
}

async function startRecording() {
  if (props.disabled || recording.value || preparing.value) return;
  preparing.value = true;
  hint.value = "正在准备录制…";
  try {
    await suspendShortcuts();
  } catch (reason) {
    preparing.value = false;
    hint.value = `无法暂停现有快捷键：${reason instanceof Error ? reason.message : String(reason)}`;
    return;
  }
  preparing.value = false;
  recording.value = true;
  validating.value = false;
  captureGeneration += 1;
  hint.value = "请按下组合键，Esc 取消";
  addCaptureListeners();
  await nextTick();
  button.value?.focus();
}

async function finishRecording() {
  captureGeneration += 1;
  recording.value = false;
  validating.value = false;
  removeCaptureListeners();
  try {
    await resumeShortcuts();
  } catch (reason) {
    hint.value = `快捷键已识别，但恢复旧快捷键失败：${reason instanceof Error ? reason.message : String(reason)}`;
  }
}

async function cancelRecording() {
  if (!recording.value) return;
  captureGeneration += 1;
  recording.value = false;
  validating.value = false;
  removeCaptureListeners();
  hint.value = "已取消";
  try {
    await resumeShortcuts();
  } catch (reason) {
    hint.value = `取消失败：${reason instanceof Error ? reason.message : String(reason)}`;
  }
  window.setTimeout(() => {
    if (!recording.value) hint.value = "";
  }, 1400);
}

async function onKeydown(event: KeyboardEvent) {
  if (!recording.value) return;
  const generation = captureGeneration;
  event.preventDefault();
  event.stopPropagation();
  event.stopImmediatePropagation();
  if (event.key == "Escape") {
    await cancelRecording();
    button.value?.blur();
    return;
  }
  if (isModifierOnly(event)) {
    hint.value = "继续按下一个普通按键";
    return;
  }
  const value = buildHotkeyFromEvent(event);
  if (!value) {
    hint.value = /^F\d+$/.test(event.code)
      ? "功能键可直接使用"
      : "请同时按下修饰键和一个主键，功能键可单独使用";
    return;
  }
  if (props.conflictValue && props.conflictValue.toLowerCase() == value.toLowerCase()) {
    hint.value = "该组合键已用于另一项功能，请换一个组合";
    return;
  }
  if (validating.value) return;
  validating.value = true;
  hint.value = `正在验证 ${formatHotkey(value, isMac.value)}…`;
  try {
    if (isTauri()) {
      await call("validate_hotkey", { value });
    }
  } catch (reason) {
    if (generation != captureGeneration || !recording.value) return;
    validating.value = false;
    hint.value = `无法使用该组合键：${reason instanceof Error ? reason.message : String(reason)}`;
    return;
  }
  if (generation != captureGeneration || !recording.value) return;
  emit("update:modelValue", value);
  hint.value = `已识别：${formatHotkey(value, isMac.value)}`;
  await finishRecording();
  window.setTimeout(() => {
    if (!recording.value) hint.value = "";
  }, 1400);
}

async function onOutsidePointerDown(event: PointerEvent) {
  if (!recording.value) return;
  const target = event.target;
  if (target instanceof Node && root.value?.contains(target)) return;
  await cancelRecording();
}

onBeforeUnmount(() => {
  removeCaptureListeners();
  if (recording.value || preparing.value) {
    void resumeShortcuts();
  }
});
</script>

<template>
  <div ref="root" class="hotkey-recorder">
    <button
      ref="button"
      type="button"
      :class="['hotkey-button', { recording }]"
      :disabled="disabled || preparing"
      :aria-label="label"
      @click="startRecording"
    >
      <span v-if="preparing" class="spinner"></span>
      <UiIcon v-else-if="recording" name="keyboard" :size="16" />
      <span v-else class="record-dot"></span>
      <strong>{{ recording ? (validating ? "正在验证…" : "请按下组合键") : displayValue }}</strong>
      <span class="record-label">{{ recording ? "Esc 取消" : "点击录制" }}</span>
    </button>
    <span class="hotkey-hint" :class="{ active: Boolean(hint) }">
      {{ hint || "点击后直接按下新的组合键，将自动识别并转换显示" }}
    </span>
  </div>
</template>

<style scoped>
.hotkey-recorder {
  display: grid;
  gap: 7px;
}

.hotkey-button {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
  width: 100%;
  min-height: 48px;
  padding: 0 12px;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: var(--bg);
  color: var(--text);
  text-align: left;
  cursor: pointer;
  transition:
    border-color 160ms ease,
    background 160ms ease;
}

.hotkey-button:hover:not(:disabled) {
  border-color: var(--line-strong);
  background: var(--bg-soft);
}

.hotkey-button.recording {
  border-color: var(--accent);
  background: var(--accent-soft);
  box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.09);
}

.hotkey-button strong {
  overflow: hidden;
  font-size: 13px;
  letter-spacing: 0.02em;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.record-dot {
  width: 8px;
  height: 8px;
  border: 1px solid var(--text-muted);
  border-radius: 50%;
}

.record-label {
  color: var(--text-muted);
  font-size: var(--font-xs);
  white-space: nowrap;
}

.hotkey-hint {
  min-height: 14px;
  color: var(--text-muted);
  font-size: var(--font-xs);
  line-height: 1.5;
}

.hotkey-hint.active {
  color: var(--accent-strong);
}
</style>
