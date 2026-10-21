<script setup lang="ts">
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import AppIcon from "../../components/AppIcon.vue";
import ProviderLogo from "../../components/ProviderLogo.vue";
import UiIcon from "../../components/UiIcon.vue";
import { inferTargetLanguage, truncate } from "../../lib/format";
import { bootstrap, call, isTauri } from "../../lib/ipc";
import type {
  PermissionStatus,
  PendingTranslation,
  ProviderResult,
  ProviderView,
  TranslationRequest,
} from "../../types";

interface ResultState {
  status: "idle" | "loading" | "success" | "error";
  result: ProviderResult | null;
  error: string;
}

const text = ref("");
const notice = ref("");
const loading = ref(true);
const providers = ref<ProviderView[]>([]);
const permissions = ref<PermissionStatus | null>(null);
const targetLang = ref("auto");
const targetMenuOpen = ref(false);
const targetPicker = ref<HTMLElement | null>(null);
const results = reactive<Record<string, ResultState>>({});
const activeRequestId = ref("");
const editingSource = ref(false);
const sourceInput = ref<HTMLTextAreaElement | null>(null);
const translateWindow = ref<HTMLElement | null>(null);
let unlistenSelection: (() => void) | undefined;
let unlistenProviders: (() => void) | undefined;
let resizeObserver: ResizeObserver | undefined;
let resizeFrame = 0;
let lastReportedHeight = 0;
let previousBodyBackground = "";

const configuredProviders = computed(() =>
  providers.value.filter((provider) => provider.enabled && provider.configured),
);

const resultList = computed(() =>
  configuredProviders.value.map((provider) => ({
    provider,
    state: results[provider.id] || { status: "idle", result: null, error: "" },
  })),
);

const firstSuccess = computed(
  () => resultList.value.find((item) => item.state.status == "success")?.state.result || null,
);
const targetOptions = [
  { value: "auto", label: "自动" },
  { value: "zh-CN", label: "简中" },
  { value: "zh-TW", label: "繁中" },
  { value: "en", label: "英语" },
  { value: "ja", label: "日语" },
  { value: "ko", label: "韩语" },
];
const targetLabel = computed(
  () => targetOptions.find((option) => option.value == targetLang.value)?.label || "自动",
);
const copySucceeded = computed(() => notice.value == "译文已复制");

const isMac = navigator.platform.toLowerCase().includes("mac");
const needsAccessibility = computed(
  () => isMac && permissions.value?.accessibility == false && notice.value.includes("辅助功能"),
);

function ensureResult(providerId: string): ResultState {
  if (!results[providerId]) {
    results[providerId] = { status: "idle", result: null, error: "" };
  }
  return results[providerId];
}

function targetFor(value: string): string {
  return inferTargetLanguage(value, targetLang.value);
}

async function focusSource() {
  editingSource.value = true;
  await nextTick();
  sourceInput.value?.focus();
}

async function loadPending(pending?: PendingTranslation) {
  loading.value = true;
  targetMenuOpen.value = false;
  try {
    const info = pending || (await call<PendingTranslation>("get_pending_translation"));
    text.value = info.text || "";
    notice.value = info.notice || "";
    editingSource.value = !text.value.trim();
    if (text.value.trim()) {
      void translateAll();
    }
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
    editingSource.value = true;
  } finally {
    loading.value = false;
    if (!text.value.trim()) await focusSource();
  }
}

async function refreshProviders() {
  const data = await bootstrap();
  providers.value = data.providers;
  permissions.value = data.permissions;
  for (const provider of configuredProviders.value) ensureResult(provider.id);
}

async function translateProvider(provider: ProviderView, requestId: string) {
  const state = ensureResult(provider.id);
  state.status = "loading";
  state.error = "";
  state.result = null;
  const request: TranslationRequest = {
    requestId,
    providerId: provider.id,
    text: text.value.trim(),
    sourceLang: "auto",
    targetLang: targetFor(text.value),
  };
  try {
    const result = await call<ProviderResult>("translate", { request });
    if (requestId != activeRequestId.value) return;
    state.result = result;
    state.status = "success";
  } catch (reason) {
    if (requestId != activeRequestId.value) return;
    state.error = reason instanceof Error ? reason.message : String(reason);
    state.status = "error";
  }
}

async function translateAll() {
  const value = text.value.trim();
  if (!value) {
    notice.value = "请选择或输入需要翻译的文本";
    await focusSource();
    return;
  }
  if (!configuredProviders.value.length) {
    notice.value = "请先在翻译插件中配置并启用至少一个服务";
    return;
  }
  const requestId = crypto.randomUUID();
  const previousRequestId = activeRequestId.value;
  activeRequestId.value = requestId;
  for (const provider of configuredProviders.value) {
    const state = ensureResult(provider.id);
    state.status = "loading";
    state.result = null;
    state.error = "";
  }
  if (previousRequestId) {
    void call("cancel_translation", { requestId: previousRequestId }).catch(() => undefined);
  }
  notice.value = "";
  await Promise.all(
    configuredProviders.value.map((provider) => translateProvider(provider, requestId)),
  );
}

async function retry(provider: ProviderView) {
  const requestId = activeRequestId.value || crypto.randomUUID();
  activeRequestId.value = requestId;
  await translateProvider(provider, requestId);
}

async function copyResult(result: ProviderResult | null) {
  if (!result) return;
  await navigator.clipboard.writeText(result.translatedText);
  notice.value = "译文已复制";
}

async function close() {
  targetMenuOpen.value = false;
  if (activeRequestId.value) {
    await call("cancel_translation", { requestId: activeRequestId.value }).catch(() => undefined);
  }
  await call("hide_translation_window").catch(() => {
    window.close();
  });
}

async function openProviderSettings() {
  await call("open_main_window", { section: "translation" });
  await close();
}

async function openAccessibilitySettings() {
  try {
    await call("open_permission_settings", { kind: "accessibility" });
    await close();
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
  }
}

function selectTarget(value: string) {
  targetLang.value = value;
  targetMenuOpen.value = false;
  translateAll();
}

function onDocumentPointerDown(event: PointerEvent) {
  if (!targetPicker.value?.contains(event.target as Node)) {
    targetMenuOpen.value = false;
  }
}

function scheduleTranslationResize() {
  if (!isTauri()) return;
  if (resizeFrame) cancelAnimationFrame(resizeFrame);
  resizeFrame = requestAnimationFrame(() => {
    resizeFrame = 0;
    const element = translateWindow.value;
    if (!element) return;
    const height = Math.max(
      220,
      Math.min(560, Math.ceil(element.getBoundingClientRect().height)),
    );
    if (Math.abs(height - lastReportedHeight) < 1) return;
    lastReportedHeight = height;
    call("resize_translation_window", { height }).catch(() => {
      // 尺寸变化失败时允许下一次内容变化继续重试。
      lastReportedHeight = 0;
    });
  });
}

function onKeydown(event: KeyboardEvent) {
  if (event.key == "Escape") {
    event.preventDefault();
    if (targetMenuOpen.value) {
      targetMenuOpen.value = false;
      return;
    }
    close();
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "c" && !window.getSelection()?.toString()) {
    if (firstSuccess.value) {
      event.preventDefault();
      copyResult(firstSuccess.value);
    }
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "r") {
    event.preventDefault();
    translateAll();
  }
}

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  document.addEventListener("pointerdown", onDocumentPointerDown);
  previousBodyBackground = document.body.style.background;
  document.body.style.background = "transparent";
  try {
    const data = await bootstrap();
    targetLang.value = data.settings.targetLang;
    providers.value = data.providers;
    permissions.value = data.permissions;
    for (const provider of configuredProviders.value) ensureResult(provider.id);
    await loadPending();
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
    loading.value = false;
    await focusSource();
  }

  if (isTauri()) {
    unlistenSelection = await listen<PendingTranslation>("translate://selection", (event) => {
      loadPending(event.payload);
    });
    unlistenProviders = await listen("app://providers-changed", () => {
      refreshProviders().catch((reason) => {
        notice.value = reason instanceof Error ? reason.message : String(reason);
      });
    });
  }
  await nextTick();
  if (isTauri() && translateWindow.value) {
    resizeObserver = new ResizeObserver(scheduleTranslationResize);
    resizeObserver.observe(translateWindow.value);
    scheduleTranslationResize();
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  document.removeEventListener("pointerdown", onDocumentPointerDown);
  unlistenSelection?.();
  unlistenProviders?.();
  resizeObserver?.disconnect();
  if (resizeFrame) cancelAnimationFrame(resizeFrame);
  document.body.style.background = previousBodyBackground;
});
</script>

<template>
  <main ref="translateWindow" class="translate-window">
    <header class="translate-head" data-tauri-drag-region>
      <div class="popover-brand" data-tauri-drag-region>
        <AppIcon :size="24" />
        <div>
          <strong>划词翻译</strong>
          <span>{{ isMac ? "⌥ D" : "Alt D" }} · 自动翻译</span>
        </div>
      </div>
      <div class="head-actions">
        <div ref="targetPicker" class="target-picker">
          <button
            type="button"
            class="target-trigger"
            :class="{ open: targetMenuOpen }"
            aria-haspopup="listbox"
            :aria-expanded="targetMenuOpen"
            @click="targetMenuOpen = !targetMenuOpen"
          >
            <UiIcon name="language" :size="13" />
            <span>{{ targetLabel }}</span>
            <UiIcon name="chevron-down" :size="11" />
          </button>
          <Transition name="target-menu">
            <div v-if="targetMenuOpen" class="target-menu" role="listbox">
              <button
                v-for="option in targetOptions"
                :key="option.value"
                type="button"
                role="option"
                :aria-selected="targetLang == option.value"
                :class="{ active: targetLang == option.value }"
                @click="selectTarget(option.value)"
              >
                <span>{{ option.label }}</span>
                <UiIcon v-if="targetLang == option.value" name="check" :size="12" />
              </button>
            </div>
          </Transition>
        </div>
        <button class="icon-button" title="翻译设置" @click="openProviderSettings">
          <UiIcon name="settings" :size="16" />
        </button>
        <button class="icon-button" title="关闭" @click="close">
          <UiIcon name="close" :size="16" />
        </button>
      </div>
    </header>

    <div v-if="loading" class="loading-row">
      <span class="spinner"></span>
      正在读取选中文本…
    </div>

    <template v-else>
      <section v-if="editingSource" class="source-panel">
        <textarea
          ref="sourceInput"
          v-model="text"
          class="source-input"
          placeholder="未读取到选中文本，可在这里手动输入…"
          @keydown.meta.enter.prevent="translateAll"
          @keydown.ctrl.enter.prevent="translateAll"
        ></textarea>
        <div class="source-actions">
          <span>{{ text.length }} 字符</span>
          <div>
            <button
              v-if="text.trim()"
              class="ghost-button compact"
              @click="editingSource = false"
            >
              收起
            </button>
            <button class="translate-button" :disabled="!text.trim()" @click="translateAll">
              <UiIcon name="translate" :size="14" />
              翻译
            </button>
          </div>
        </div>
      </section>

      <section v-else class="source-preview" @dblclick="focusSource">
        <p>{{ truncate(text, 220) }}</p>
        <button class="ghost-button compact" @click="focusSource">编辑原文</button>
      </section>

      <div v-if="notice" :class="['notice', { success: copySucceeded }]">
        <UiIcon :name="copySucceeded ? 'check' : 'alert'" :size="14" />
        <span>{{ notice }}</span>
        <button v-if="needsAccessibility" class="notice-action" @click="openAccessibilitySettings">
          去授权
        </button>
      </div>

      <section v-if="configuredProviders.length" class="results">
        <article v-for="{ provider, state } in resultList" :key="provider.id" class="result-card">
          <div class="result-head">
            <div class="provider-name">
              <ProviderLogo :kind="provider.kind" :size="26" />
              <div>
                <strong>{{ provider.name }}</strong>
                <span v-if="state.result">
                  {{ state.result.detectedLang || "自动" }} → {{ targetFor(text) }} ·
                  {{ state.result.elapsedMs }}ms
                </span>
                <span v-else>{{ provider.model || provider.kind }}</span>
              </div>
            </div>
            <div class="result-actions">
              <button
                v-if="state.status == 'success'"
                class="icon-button"
                title="复制译文"
                @click="copyResult(state.result)"
              >
                <UiIcon name="copy" :size="15" />
              </button>
              <button
                v-if="state.status == 'error'"
                class="icon-button"
                title="重试"
                @click="retry(provider)"
              >
                <UiIcon name="update" :size="15" />
              </button>
            </div>
          </div>

          <div v-if="state.status == 'loading'" class="result-loading">
            <span class="spinner"></span>
            翻译中…
          </div>
          <div v-else-if="state.status == 'error'" class="result-error">
            <UiIcon name="alert" :size="15" />
            <span>{{ state.error }}</span>
          </div>
          <p v-else-if="state.result" class="result-text">{{ state.result.translatedText }}</p>
          <p v-else class="result-placeholder">等待翻译</p>
        </article>
      </section>

      <section v-else class="provider-empty">
        <UiIcon name="provider" :size="20" />
        <div>
          <strong>还没有可用的翻译服务</strong>
          <span>先在翻译插件中完成配置并启用服务。</span>
        </div>
        <button class="ghost-button compact" @click="openProviderSettings">去配置</button>
      </section>
    </template>

    <footer class="translate-foot">
      <span>Esc 关闭 · 点击外部隐藏</span>
      <button v-if="firstSuccess" class="ghost-button compact" @click="copyResult(firstSuccess)">
        <UiIcon name="copy" :size="13" />
        复制译文
      </button>
    </footer>
  </main>
</template>

<style scoped>
.translate-window {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: auto;
  overflow: hidden;
  border: 1px solid var(--line-strong);
  border-radius: 14px;
  background:
    linear-gradient(180deg, rgba(43, 44, 48, 0.97), rgba(32, 33, 36, 0.99));
  box-shadow: none;
  backdrop-filter: blur(22px) saturate(1.12);
}

.translate-head {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-height: 52px;
  padding: 9px 10px;
  border-bottom: 1px solid var(--line);
}

.popover-brand,
.head-actions,
.source-actions,
.source-actions > div,
.provider-name,
.result-head,
.result-actions,
.translate-foot {
  display: flex;
  align-items: center;
}

.popover-brand {
  min-width: 0;
  gap: 7px;
}

.popover-brand > div {
  min-width: 0;
}

.popover-brand strong,
.popover-brand span {
  display: block;
}

.popover-brand strong {
  font-size: 16px;
  letter-spacing: -0.02em;
}

.popover-brand span {
  margin-top: 1px;
  overflow: hidden;
  color: var(--text-muted);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.head-actions {
  flex: 0 0 auto;
  gap: 2px;
}

.target-picker {
  position: relative;
  flex: 0 0 auto;
}

.target-trigger {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.045);
  color: var(--text-soft);
  font-size: 12px;
  cursor: pointer;
  transition:
    border-color 140ms ease,
    background 140ms ease,
    color 140ms ease;
}

.target-trigger:hover,
.target-trigger.open {
  border-color: var(--line-strong);
  background: rgba(255, 255, 255, 0.08);
  color: var(--text);
}

.target-trigger > span {
  min-width: 24px;
  text-align: left;
}

.target-menu {
  position: absolute;
  z-index: 20;
  top: calc(100% + 6px);
  right: 0;
  width: 112px;
  padding: 4px;
  border: 1px solid var(--line-strong);
  border-radius: 10px;
  background: rgba(45, 46, 50, 0.98);
  box-shadow: 0 14px 34px rgba(0, 0, 0, 0.34);
  backdrop-filter: blur(18px);
}

.target-menu button {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  height: 30px;
  padding: 0 8px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-soft);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}

.target-menu button:hover {
  background: rgba(255, 255, 255, 0.065);
  color: var(--text);
}

.target-menu button.active {
  background: var(--accent-soft);
  color: var(--accent-strong);
  font-weight: 650;
}

.target-menu-enter-active,
.target-menu-leave-active {
  transition:
    opacity 120ms ease,
    transform 120ms ease;
}

.target-menu-enter-from,
.target-menu-leave-to {
  opacity: 0;
  transform: translateY(-4px) scale(0.98);
}

.icon-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-soft);
  cursor: pointer;
}

.icon-button:hover {
  background: var(--bg-soft);
  color: var(--text);
}

.loading-row {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 110px;
  color: var(--text-muted);
  font-size: 12px;
}

.source-panel,
.source-preview {
  flex: 0 0 auto;
  margin: 10px 10px 0;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.035);
}

.source-panel {
  padding: 10px;
}

.source-input {
  width: 100%;
  min-height: 76px;
  max-height: 132px;
  padding: 0;
  border: 0;
  outline: 0;
  resize: none;
  background: transparent;
  color: var(--text);
  font-size: 14px;
  line-height: 1.6;
}

.source-input::placeholder {
  color: var(--text-muted);
}

.source-actions {
  justify-content: space-between;
  gap: 8px;
  margin-top: 8px;
  color: var(--text-muted);
  font-size: 12px;
}

.source-actions > div {
  gap: 5px;
}

.translate-button {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 30px;
  padding: 0 11px;
  border: 0;
  border-radius: 8px;
  background: var(--text);
  color: #18181a;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
}

.translate-button:disabled {
  opacity: 0.45;
  cursor: default;
}

.source-preview {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px;
}

.source-preview p {
  display: -webkit-box;
  min-width: 0;
  flex: 1;
  margin: 0;
  overflow: hidden;
  color: var(--text-soft);
  font-size: 13px;
  line-height: 1.55;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.ghost-button.compact {
  min-height: 28px;
  padding: 0 9px;
  font-size: 12px;
}

.notice {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 7px;
  margin: 9px 10px 0;
  padding: 9px 10px;
  border: 1px solid rgba(248, 113, 113, 0.16);
  border-radius: 9px;
  background: rgba(248, 113, 113, 0.1);
  color: var(--danger);
  font-size: 12px;
  line-height: 1.45;
}

.notice.success {
  border-color: rgba(74, 222, 128, 0.18);
  background: rgba(74, 222, 128, 0.09);
  color: var(--success);
}

.notice > span {
  min-width: 0;
  flex: 1;
}

.notice-action {
  flex: 0 0 auto;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font-size: inherit;
  font-weight: 700;
  text-decoration: underline;
  cursor: pointer;
}

.results {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 300px;
  overflow-y: auto;
  padding: 10px;
}

.result-card {
  flex: 0 0 auto;
  padding: 11px;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.04);
}

.result-head {
  justify-content: space-between;
  gap: 8px;
}

.provider-name {
  min-width: 0;
  gap: 8px;
}

.provider-name strong,
.provider-name span {
  display: block;
}

.provider-name strong {
  font-size: 14px;
}

.provider-name span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 12px;
}

.result-actions {
  gap: 1px;
}

.result-loading,
.result-error {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 9px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.45;
}

.result-error {
  color: var(--danger);
}

.result-text {
  margin: 9px 0 0;
  color: var(--text);
  font-size: 14px;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
}

.result-placeholder {
  margin: 9px 0 0;
  color: var(--text-muted);
  font-size: 12px;
}

.provider-empty {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  margin: 10px;
  padding: 13px;
  border: 1px dashed var(--line-strong);
  border-radius: 11px;
  color: var(--text-muted);
}

.provider-empty div {
  min-width: 0;
  flex: 1;
}

.provider-empty strong,
.provider-empty span {
  display: block;
}

.provider-empty strong {
  color: var(--text);
  font-size: 14px;
}

.provider-empty span {
  margin-top: 3px;
  font-size: 12px;
  line-height: 1.45;
}

.translate-foot {
  flex: 0 0 auto;
  justify-content: space-between;
  gap: 8px;
  min-height: 40px;
  padding: 6px 10px;
  border-top: 1px solid var(--line);
  color: var(--text-muted);
  font-size: 12px;
}

.result-loading .spinner,
.loading-row .spinner {
  width: 12px;
  height: 12px;
  border-width: 2px;
  border-color: rgba(255, 255, 255, 0.15);
  border-top-color: var(--accent);
}
</style>
