<script setup lang="ts">
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import AppIcon from "../../components/AppIcon.vue";
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
const results = reactive<Record<string, ResultState>>({});
const activeRequestId = ref("");
const editingSource = ref(false);
const sourceInput = ref<HTMLTextAreaElement | null>(null);
let unlistenSelection: (() => void) | undefined;

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
  try {
    const info = pending || (await call<PendingTranslation>("get_pending_translation"));
    text.value = info.text || "";
    notice.value = info.notice || "";
    editingSource.value = !text.value.trim();
    if (text.value.trim()) {
      await translateAll();
    }
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
    editingSource.value = true;
  } finally {
    loading.value = false;
    if (!text.value.trim()) await focusSource();
  }
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
    state.result = await call<ProviderResult>("translate", { request });
    state.status = "success";
  } catch (reason) {
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
  if (activeRequestId.value) {
    await call("cancel_translation", { requestId: activeRequestId.value }).catch(() => undefined);
  }
  notice.value = "";
  const requestId = crypto.randomUUID();
  activeRequestId.value = requestId;
  await Promise.all(
    configuredProviders.value.map((provider) => translateProvider(provider, requestId)),
  );
}

async function retry(provider: ProviderView) {
  const requestId = crypto.randomUUID();
  activeRequestId.value = requestId;
  await translateProvider(provider, requestId);
}

async function copyResult(result: ProviderResult | null) {
  if (!result) return;
  await navigator.clipboard.writeText(result.translatedText);
  notice.value = "译文已复制";
}

async function close() {
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

function onKeydown(event: KeyboardEvent) {
  if (event.key == "Escape") {
    event.preventDefault();
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
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenSelection?.();
});
</script>

<template>
  <main class="translate-window">
    <header class="translate-head" data-tauri-drag-region>
      <div class="popover-brand" data-tauri-drag-region>
        <AppIcon :size="28" />
        <div>
          <strong>划词翻译</strong>
          <span>{{ isMac ? "⌥ D" : "Alt D" }} · 自动翻译</span>
        </div>
      </div>
      <div class="head-actions">
        <label class="target-picker">
          <UiIcon name="language" :size="13" />
          <select v-model="targetLang" @change="translateAll">
            <option value="auto">自动</option>
            <option value="zh-CN">简中</option>
            <option value="zh-TW">繁中</option>
            <option value="en">英语</option>
            <option value="ja">日语</option>
            <option value="ko">韩语</option>
          </select>
        </label>
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

      <div v-if="notice" class="notice">
        <UiIcon name="alert" :size="14" />
        <span>{{ notice }}</span>
        <button v-if="needsAccessibility" class="notice-action" @click="openAccessibilitySettings">
          去授权
        </button>
      </div>

      <section v-if="configuredProviders.length" class="results">
        <article v-for="{ provider, state } in resultList" :key="provider.id" class="result-card">
          <div class="result-head">
            <div class="provider-name">
              <span class="provider-mark">{{ provider.name.slice(0, 1) }}</span>
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
  height: 100%;
  overflow: hidden;
  border: 1px solid var(--line-strong);
  border-radius: 12px;
  background: color-mix(in srgb, var(--bg-elevated) 98%, transparent);
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.42);
}

.translate-head {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-height: 46px;
  padding: 8px 10px;
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
  gap: 8px;
}

.popover-brand strong,
.popover-brand span {
  display: block;
}

.popover-brand strong {
  font-size: 11px;
}

.popover-brand span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 8px;
}

.head-actions {
  gap: 3px;
}

.target-picker {
  display: flex;
  align-items: center;
  gap: 3px;
  height: 27px;
  padding: 0 5px;
  border: 1px solid var(--line);
  border-radius: 7px;
  color: var(--text-soft);
}

.target-picker select {
  max-width: 60px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text-soft);
  font-size: 9px;
}

.icon-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 27px;
  height: 27px;
  padding: 0;
  border: 0;
  border-radius: 7px;
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
  flex: 1;
  align-items: center;
  justify-content: center;
  gap: 8px;
  color: var(--text-muted);
  font-size: 10px;
}

.source-panel,
.source-preview {
  flex: 0 0 auto;
  margin: 8px 10px 0;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg);
}

.source-panel {
  padding: 7px 9px;
}

.source-input {
  width: 100%;
  min-height: 58px;
  max-height: 104px;
  padding: 0;
  border: 0;
  outline: 0;
  resize: none;
  background: transparent;
  color: var(--text);
  font-size: 12px;
  line-height: 1.55;
}

.source-input::placeholder {
  color: var(--text-muted);
}

.source-actions {
  justify-content: space-between;
  gap: 8px;
  margin-top: 5px;
  color: var(--text-muted);
  font-size: 8px;
}

.source-actions > div {
  gap: 5px;
}

.translate-button {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 26px;
  padding: 0 8px;
  border: 0;
  border-radius: 7px;
  background: var(--accent);
  color: #1c1307;
  font-size: 9px;
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
  padding: 7px 9px;
}

.source-preview p {
  display: -webkit-box;
  min-width: 0;
  flex: 1;
  margin: 0;
  overflow: hidden;
  color: var(--text-soft);
  font-size: 10px;
  line-height: 1.45;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}

.ghost-button.compact {
  min-height: 24px;
  padding: 0 7px;
  font-size: 8px;
}

.notice {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 6px;
  margin: 7px 10px 0;
  padding: 7px 9px;
  border-radius: 7px;
  background: rgba(255, 107, 104, 0.09);
  color: var(--danger);
  font-size: 8px;
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
  min-height: 0;
  flex: 1;
  flex-direction: column;
  gap: 6px;
  overflow-y: auto;
  padding: 8px 10px;
}

.result-card {
  flex: 0 0 auto;
  padding: 9px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--bg);
}

.result-head {
  justify-content: space-between;
  gap: 8px;
}

.provider-name {
  min-width: 0;
  gap: 7px;
}

.provider-mark {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 7px;
  background: var(--bg-soft);
  color: var(--accent-strong);
  font-size: 9px;
  font-weight: 750;
}

.provider-name strong,
.provider-name span {
  display: block;
}

.provider-name strong {
  font-size: 10px;
}

.provider-name span {
  margin-top: 1px;
  color: var(--text-muted);
  font-size: 8px;
}

.result-actions {
  gap: 1px;
}

.result-loading,
.result-error {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 7px;
  color: var(--text-muted);
  font-size: 9px;
}

.result-error {
  color: var(--danger);
}

.result-text {
  margin: 7px 0 0;
  color: var(--text);
  font-size: 12px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.result-placeholder {
  margin: 7px 0 0;
  color: var(--text-muted);
  font-size: 9px;
}

.provider-empty {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 9px;
  margin: 8px 10px;
  padding: 12px;
  border: 1px dashed var(--line-strong);
  border-radius: 9px;
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
  color: var(--text-soft);
  font-size: 10px;
}

.provider-empty span {
  margin-top: 2px;
  font-size: 8px;
}

.translate-foot {
  flex: 0 0 auto;
  justify-content: space-between;
  gap: 8px;
  min-height: 32px;
  padding: 4px 10px;
  border-top: 1px solid var(--line);
  color: var(--text-muted);
  font-size: 8px;
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
