<script setup lang="ts">
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import AppIcon from "../../components/AppIcon.vue";
import UiIcon from "../../components/UiIcon.vue";
import { inferTargetLanguage, truncate } from "../../lib/format";
import { bootstrap, call, isTauri } from "../../lib/ipc";
import type {
  AppSettings,
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
const selectedProviderIds = ref<string[]>([]);
const settings = ref<AppSettings | null>(null);
const targetLang = ref("auto");
const results = reactive<Record<string, ResultState>>({});
const activeRequestId = ref("");
const composerFocused = ref(false);
let unlistenSelection: (() => void) | undefined;

const configuredProviders = computed(() =>
  providers.value.filter((provider) => provider.enabled && provider.configured),
);

const resultList = computed(() =>
  providers.value
    .filter((provider) => selectedProviderIds.value.includes(provider.id))
    .map((provider) => ({
      provider,
      state: results[provider.id] || { status: "idle", result: null, error: "" },
    })),
);

const isMac = navigator.platform.toLowerCase().includes("mac");

function ensureResult(providerId: string): ResultState {
  if (!results[providerId]) {
    results[providerId] = { status: "idle", result: null, error: "" };
  }
  return results[providerId];
}

function targetFor(value: string): string {
  return inferTargetLanguage(value, targetLang.value);
}

async function loadPending(pending?: PendingTranslation) {
  try {
    const info = pending || (await call<PendingTranslation>("get_pending_translation"));
    text.value = info.text || "";
    notice.value = info.notice || "";
    if (text.value.trim()) {
      await translateAll();
    }
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    loading.value = false;
  }
}

function toggleProvider(providerId: string) {
  if (selectedProviderIds.value.includes(providerId)) {
    if (selectedProviderIds.value.length == 1) return;
    selectedProviderIds.value = selectedProviderIds.value.filter((id) => id != providerId);
  } else {
    selectedProviderIds.value = [...selectedProviderIds.value, providerId];
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
    notice.value = "请输入或选中需要翻译的文本";
    return;
  }
  if (!selectedProviderIds.value.length) {
    notice.value = configuredProviders.value.length
      ? "请至少选择一个翻译服务"
      : "请先在设置中配置并启用至少一个翻译服务";
    return;
  }
  if (activeRequestId.value) {
    await call("cancel_translation", { requestId: activeRequestId.value }).catch(() => undefined);
  }
  notice.value = "";
  const requestId = crypto.randomUUID();
  activeRequestId.value = requestId;
  const selected = providers.value.filter((provider) =>
    selectedProviderIds.value.includes(provider.id),
  );
  await Promise.all(selected.map((provider) => translateProvider(provider, requestId)));
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

function onKeydown(event: KeyboardEvent) {
  if (event.key == "Escape") {
    event.preventDefault();
    close();
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "c" && !window.getSelection()?.toString()) {
    const success = resultList.value.find((item) => item.state.status == "success");
    if (success) {
      event.preventDefault();
      copyResult(success.state.result);
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
    settings.value = data.settings;
    targetLang.value = data.settings.targetLang;
    providers.value = data.providers;
    selectedProviderIds.value = configuredProviders.value.map((provider) => provider.id);
    for (const provider of configuredProviders.value) ensureResult(provider.id);
    await loadPending();
  } catch (reason) {
    notice.value = reason instanceof Error ? reason.message : String(reason);
    loading.value = false;
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
      <div class="brand" data-tauri-drag-region>
        <AppIcon :size="30" />
        <div>
          <strong>S-Tools</strong>
          <span>划词翻译</span>
        </div>
      </div>
      <div class="head-actions">
        <span class="badge"><UiIcon name="keyboard" :size="12" /> Esc 关闭</span>
        <button class="icon-button" title="设置翻译服务" @click="openProviderSettings">
          <UiIcon name="settings" />
        </button>
        <button class="icon-button" title="关闭" @click="close">
          <UiIcon name="close" />
        </button>
      </div>
    </header>

    <section class="source-panel">
      <textarea
        v-model="text"
        class="source-input"
        :class="{ focused: composerFocused }"
        :placeholder="loading ? '正在读取选中文本…' : '选中文本，或直接在这里输入…'"
        @focus="composerFocused = true"
        @blur="composerFocused = false"
        @keydown.meta.enter.prevent="translateAll"
        @keydown.ctrl.enter.prevent="translateAll"
      ></textarea>
      <div class="source-foot">
        <span>{{ text.length }} 字符</span>
        <div>
          <label class="language-picker">
            <UiIcon name="language" :size="14" />
            <select v-model="targetLang">
              <option value="auto">自动目标</option>
              <option value="zh-CN">简体中文</option>
              <option value="zh-TW">繁体中文</option>
              <option value="en">英语</option>
              <option value="ja">日语</option>
              <option value="ko">韩语</option>
            </select>
          </label>
          <button class="translate-button" :disabled="!text.trim()" @click="translateAll">
            <UiIcon name="translate" :size="15" />
            {{ isMac ? "⌘ ↵" : "Ctrl ↵" }}
          </button>
        </div>
      </div>
    </section>

    <section v-if="configuredProviders.length" class="provider-strip">
      <span>对照服务</span>
      <button
        v-for="provider in configuredProviders"
        :key="provider.id"
        :class="['provider-chip', { active: selectedProviderIds.includes(provider.id) }]"
        @click="toggleProvider(provider.id)"
      >
        <span :class="{ dot: true, active: selectedProviderIds.includes(provider.id) }"></span>
        {{ provider.name }}
      </button>
    </section>
    <section v-else class="provider-empty">
      <UiIcon name="provider" />
      <span>还没有可用的翻译服务</span>
      <button class="ghost-button" @click="openProviderSettings">现在配置</button>
    </section>

    <div v-if="notice" class="notice">
      <UiIcon name="alert" :size="15" />
      {{ notice }}
    </div>

    <section class="results" :class="{ single: resultList.length == 1 }">
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
              <UiIcon name="copy" :size="16" />
            </button>
            <button
              v-if="state.status == 'error'"
              class="icon-button"
              title="重试"
              @click="retry(provider)"
            >
              <UiIcon name="update" :size="16" />
            </button>
          </div>
        </div>

        <div v-if="state.status == 'loading'" class="result-loading">
          <span class="spinner"></span>
          <span>翻译中…</span>
        </div>
        <div v-else-if="state.status == 'error'" class="result-error">
          <UiIcon name="alert" :size="16" />
          <span>{{ state.error }}</span>
        </div>
        <p v-else-if="state.result" class="result-text">{{ state.result.translatedText }}</p>
        <p v-else class="result-placeholder">等待翻译</p>
      </article>
    </section>

    <footer class="translate-foot">
      <span>
        <UiIcon name="shield" :size="13" />
        文本只会发送到已选中的服务
      </span>
      <button class="ghost-button" @click="close">关闭窗口</button>
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
  border-radius: 18px;
  background: color-mix(in srgb, var(--bg-elevated) 97%, transparent);
  box-shadow: 0 30px 90px rgba(0, 0, 0, 0.42);
}

.translate-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--line);
}

.brand,
.head-actions,
.source-foot,
.source-foot > div,
.provider-strip,
.provider-name,
.result-head,
.result-actions,
.translate-foot {
  display: flex;
  align-items: center;
}

.brand {
  gap: 9px;
}

.brand strong,
.brand span {
  display: block;
}

.brand strong {
  font-size: 12px;
}

.brand span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 9px;
}

.head-actions {
  gap: 5px;
}

.icon-button {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
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

.source-panel {
  margin: 14px 14px 0;
  padding: 14px;
  border: 1px solid var(--line);
  border-radius: 13px;
  background: var(--bg);
}

.source-input {
  width: 100%;
  min-height: 92px;
  max-height: 170px;
  padding: 0;
  border: 0;
  outline: 0;
  resize: vertical;
  background: transparent;
  color: var(--text);
  font-size: 15px;
  line-height: 1.7;
}

.source-input::placeholder {
  color: var(--text-muted);
}

.source-foot {
  justify-content: space-between;
  gap: 16px;
  margin-top: 9px;
  color: var(--text-muted);
  font-size: 9px;
}

.source-foot > div {
  gap: 9px;
}

.language-picker {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--text-soft);
}

.language-picker select {
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--text-soft);
  font-size: 10px;
}

.translate-button {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 29px;
  padding: 0 10px;
  border: 0;
  border-radius: 8px;
  background: var(--accent);
  color: #1c1307;
  font-size: 10px;
  font-weight: 700;
  cursor: pointer;
}

.provider-strip,
.provider-empty {
  margin: 10px 14px 0;
}

.provider-strip {
  gap: 8px;
  overflow-x: auto;
}

.provider-strip > span {
  flex: 0 0 auto;
  color: var(--text-muted);
  font-size: 9px;
}

.provider-chip {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 5px;
  height: 25px;
  padding: 0 8px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: transparent;
  color: var(--text-muted);
  font-size: 9px;
  cursor: pointer;
}

.provider-chip.active {
  border-color: rgba(247, 147, 30, 0.28);
  background: var(--accent-soft);
  color: var(--accent-strong);
}

.dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-muted);
}

.dot.active {
  background: var(--accent);
}

.provider-empty {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border: 1px dashed var(--line-strong);
  border-radius: 10px;
  color: var(--text-muted);
  font-size: 10px;
}

.provider-empty .ghost-button {
  min-height: 25px;
  margin-left: auto;
  padding: 0 7px;
  font-size: 9px;
}

.notice {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: 10px 14px 0;
  padding: 9px 11px;
  border-radius: 9px;
  background: rgba(255, 107, 104, 0.09);
  color: var(--danger);
  font-size: 9px;
}

.results {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 9px;
  min-height: 0;
  overflow-y: auto;
  padding: 11px 14px;
}

.results.single {
  grid-template-columns: 1fr;
}

.result-card {
  display: flex;
  flex-direction: column;
  min-height: 150px;
  padding: 13px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--bg);
}

.result-head {
  justify-content: space-between;
  gap: 10px;
}

.provider-name {
  gap: 8px;
}

.provider-mark {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 27px;
  height: 27px;
  border-radius: 8px;
  background: var(--bg-soft);
  color: var(--accent-strong);
  font-size: 10px;
  font-weight: 750;
}

.provider-name strong,
.provider-name span {
  display: block;
}

.provider-name strong {
  font-size: 10px;
}

.provider-name div > span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 8px;
}

.result-actions {
  gap: 2px;
}

.result-actions .icon-button {
  width: 26px;
  height: 26px;
}

.result-text {
  margin: 13px 0 0;
  overflow-y: auto;
  color: var(--text);
  font-size: 12px;
  line-height: 1.75;
  white-space: pre-wrap;
}

.result-placeholder {
  margin: 18px 0 0;
  color: var(--text-muted);
  font-size: 10px;
}

.result-loading,
.result-error {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 18px;
  color: var(--text-muted);
  font-size: 10px;
}

.result-error {
  align-items: flex-start;
  color: var(--danger);
  line-height: 1.5;
}

.translate-foot {
  justify-content: space-between;
  gap: 12px;
  margin-top: auto;
  padding: 9px 14px 11px;
  border-top: 1px solid var(--line);
  color: var(--text-muted);
  font-size: 8px;
}

.translate-foot > span {
  display: flex;
  align-items: center;
  gap: 5px;
}

.translate-foot .ghost-button {
  min-height: 24px;
  padding: 0 5px;
  font-size: 8px;
}

@media (max-width: 640px) {
  .results {
    grid-template-columns: 1fr;
  }
}
</style>
