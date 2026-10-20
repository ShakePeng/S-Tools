<script setup lang="ts">
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import AppIcon from "../components/AppIcon.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import UiIcon from "../components/UiIcon.vue";
import { formatDate, truncate } from "../lib/format";
import { formatHotkey } from "../lib/hotkeys";
import { assetUrl, bootstrap, call, isTauri } from "../lib/ipc";
import type {
  AppSettings,
  BootstrapData,
  HistoryItem,
  ProviderInput,
  ProviderResult,
  ProviderView,
  SettingsInput,
  UpdateInfo,
  UpdateInstallResult,
  UpdateProgressEvent,
} from "../types";

type Section = "home" | "history" | "translation" | "screenshot" | "settings" | "updates";

const sections: Array<{ id: Section; label: string; icon: string }> = [
  { id: "home", label: "工作台", icon: "home" },
  { id: "history", label: "历史记录", icon: "history" },
  { id: "translation", label: "翻译", icon: "translate" },
  { id: "screenshot", label: "截图", icon: "screenshot" },
  { id: "settings", label: "通用设置", icon: "settings" },
  { id: "updates", label: "软件更新", icon: "update" },
];

const rawRequestedSection = new URLSearchParams(window.location.search).get("section");
const requestedSection = rawRequestedSection == "providers" ? "translation" : rawRequestedSection;
const section = ref<Section>(
  sections.some((item) => item.id === requestedSection)
    ? (requestedSection as Section)
    : "home",
);
const loading = ref(true);
const data = ref<BootstrapData | null>(null);
const error = ref("");
const toast = ref("");
const historyTab = ref<"translation" | "screenshot">("translation");
const editing = ref<ProviderView | null>(null);
const credentials = reactive<Record<string, string>>({});
const providerSaving = ref(false);
const settingsSaving = ref(false);
const featureSaving = ref(false);
const updateChecking = ref(false);
const updateInstalling = ref(false);
const updateInfo = ref<UpdateInfo | null>(null);
const updateProgress = ref(0);
const updateError = ref("");
const updateMessage = ref("");
const settingsForm = reactive<SettingsInput>({
  schemaVersion: 1,
  hotkeys: { translate: "Alt+D", screenshot: "F1" },
  translationEnabled: false,
  screenshotEnabled: false,
  targetLang: "auto",
  launchAtLogin: false,
  historyLimit: 200,
  theme: "dark",
  hasCompletedOnboarding: true,
});

let toastTimer: number | undefined;
let unlistenUpdate: (() => void) | undefined;
let unlistenNavigate: (() => void) | undefined;
let unlistenToast: (() => void) | undefined;
let unlistenHistory: (() => void) | undefined;

const enabledProviders = computed(
  () => data.value?.providers.filter((provider) => provider.enabled && provider.configured) || [],
);
const isMac = navigator.platform.toLowerCase().includes("mac");
const insecureBaseUrl = computed(
  () =>
    ["deepseek", "openai-compatible"].includes(editing.value?.kind || "") &&
    editing.value?.baseUrl.trim().toLowerCase().startsWith("http://"),
);
const visibleSections = computed(() =>
  sections.filter((item) => {
    if (item.id == "translation") return settingsForm.translationEnabled;
    if (item.id == "screenshot") return settingsForm.screenshotEnabled;
    return true;
  }),
);

const currentHistory = computed<HistoryItem[]>(() => {
  if (!data.value) return [];
  return historyTab.value === "translation"
    ? data.value.translationHistory
    : data.value.screenshotHistory;
});
const recentScreenshots = computed(() => data.value?.screenshotHistory.slice(0, 6) || []);

function notify(message: string) {
  toast.value = message;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toast.value = "";
  }, 2600);
}

function applyTheme(theme: AppSettings["theme"]) {
  const resolved =
    theme === "system"
      ? window.matchMedia("(prefers-color-scheme: light)").matches
        ? "light"
        : "dark"
      : theme;
  document.documentElement.dataset.theme = resolved;
}

function syncSettingsForm(settings: AppSettings) {
  Object.assign(settingsForm, JSON.parse(JSON.stringify(settings)));
  applyTheme(settings.theme);
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await bootstrap();
    syncSettingsForm(data.value.settings);
    if (section.value == "translation" && !data.value.settings.translationEnabled) {
      section.value = "home";
    }
    if (section.value == "screenshot" && !data.value.settings.screenshotEnabled) {
      section.value = "home";
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    loading.value = false;
  }
}

async function toggleFeature(kind: "translation" | "screenshot") {
  if (featureSaving.value) return;
  featureSaving.value = true;
  const field = kind == "translation" ? "translationEnabled" : "screenshotEnabled";
  const previous = settingsForm[field];
  settingsForm[field] = !previous;
  try {
    const saved = await call<AppSettings>("save_settings", { input: settingsForm });
    if (data.value) data.value.settings = saved;
    syncSettingsForm(saved);
    if (!saved[field] && section.value == kind) section.value = "home";
    notify(`${kind == "translation" ? "划词翻译" : "截图"}已${saved[field] ? "开启" : "关闭"}`);
  } catch (reason) {
    settingsForm[field] = previous;
    notify(reason instanceof Error ? reason.message : String(reason));
  } finally {
    featureSaving.value = false;
  }
}

async function refreshHistory() {
  if (!data.value || !isTauri()) return;
  const history = await call<{ translations: HistoryItem[]; screenshots: HistoryItem[] }>(
    "get_all_history",
  );
  data.value.translationHistory = history.translations;
  data.value.screenshotHistory = history.screenshots;
}

async function saveSettings() {
  settingsSaving.value = true;
  try {
    const settings = await call<AppSettings>("save_settings", { input: settingsForm });
    if (data.value) data.value.settings = settings;
    syncSettingsForm(settings);
    notify("设置已保存");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  } finally {
    settingsSaving.value = false;
  }
}

async function startTranslation() {
  try {
    await call("open_translation");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function startCapture() {
  try {
    await call("start_capture");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

function openProvider(provider: ProviderView) {
  editing.value = provider;
  for (const key of Object.keys(credentials)) delete credentials[key];
  credentials.appId = "";
  credentials.secret = "";
  credentials.apiKey = "";
  credentials.secretId = "";
  credentials.secretKey = "";
}

async function persistProvider(closeAfterSave: boolean): Promise<boolean> {
  if (!editing.value) return false;
  providerSaving.value = true;
  try {
    const input: ProviderInput = {
      id: editing.value.id,
      name: editing.value.name,
      kind: editing.value.kind,
      enabled: editing.value.enabled,
      model: editing.value.model,
      baseUrl: editing.value.baseUrl,
      credentials: { ...credentials },
    };
    const provider = await call<ProviderView>("save_provider", { input });
    if (data.value) {
      const index = data.value.providers.findIndex((item) => item.id == provider.id);
      if (index >= 0) data.value.providers[index] = provider;
    }
    if (closeAfterSave) editing.value = null;
    notify("翻译服务已保存");
    return true;
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
    return false;
  } finally {
    providerSaving.value = false;
  }
}

async function saveProvider() {
  await persistProvider(true);
}

async function testProvider() {
  const target = editing.value;
  if (!target) return;
  const saved = await persistProvider(false);
  if (!saved) return;
  providerSaving.value = true;
  try {
    const result = await call<ProviderResult>("test_provider", { providerId: target.id });
    notify(`连接成功：${truncate(result.translatedText, 36)}`);
  } catch (reason) {
    notify(`连接失败：${reason instanceof Error ? reason.message : String(reason)}`);
  } finally {
    providerSaving.value = false;
  }
}

async function toggleProvider(provider: ProviderView) {
  provider.enabled = !provider.enabled;
  if (!isTauri()) return;
  try {
    const updated = await call<ProviderView>("set_provider_enabled", {
      id: provider.id,
      enabled: provider.enabled,
    });
    Object.assign(provider, updated);
  } catch (reason) {
    provider.enabled = !provider.enabled;
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function openPermission(kind: "accessibility" | "screenRecording") {
  try {
    await call("open_permission_settings", { kind });
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function copyHistory(item: HistoryItem) {
  try {
    if (item.kind === "screenshot" && item.path && isTauri()) {
      await call("copy_history_image", { path: item.path });
    } else {
      await navigator.clipboard.writeText(item.subtitle);
    }
    notify("已复制到剪贴板");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function deleteHistory(item: HistoryItem) {
  try {
    await call("delete_history", { kind: item.kind, id: item.id });
    if (data.value) {
      if (item.kind === "translation") {
        data.value.translationHistory = data.value.translationHistory.filter(
          (entry) => entry.id !== item.id,
        );
      } else {
        data.value.screenshotHistory = data.value.screenshotHistory.filter(
          (entry) => entry.id !== item.id,
        );
      }
    }
    notify("记录已删除");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function checkUpdate() {
  updateChecking.value = true;
  updateError.value = "";
  updateMessage.value = "";
  try {
    const info = await call<UpdateInfo | null>("check_update");
    updateInfo.value = info;
    updateMessage.value = info ? `发现新版本 ${info.version}` : "当前已经是最新版本";
  } catch (reason) {
    updateError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    updateChecking.value = false;
  }
}

async function installUpdate() {
  if (!updateInfo.value) return;
  updateInstalling.value = true;
  updateError.value = "";
  updateProgress.value = 0;
  try {
    const channel = new Channel<UpdateProgressEvent>();
    let downloaded = 0;
    let total = 0;
    channel.onmessage = (event) => {
      if (event.event == "Started") total = event.contentLength || 0;
      if (event.event == "Progress") {
        downloaded += event.chunkLength || 0;
        updateProgress.value = total ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
      }
    };
    const result = await call<UpdateInstallResult>("install_update", {
      url: updateInfo.value.downloadUrl,
      fileName: updateInfo.value.fileName,
      sha256: updateInfo.value.sha256,
      onEvent: channel,
    });
    if (result.installed) {
      updateMessage.value = "正在安装并重启…";
      return;
    }
    if (updateInfo.value.manualDownloadUrl) {
      const path = await call<string>("download_update", {
        url: updateInfo.value.manualDownloadUrl,
        fileName: updateInfo.value.manualFileName || updateInfo.value.fileName,
        onEvent: channel,
      });
      await call("reveal_path", { path });
      updateMessage.value = "安装包已下载，请手动完成安装";
    }
  } catch (reason) {
    updateError.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    updateInstalling.value = false;
  }
}

onMounted(async () => {
  await load();
  if (isTauri()) {
    unlistenUpdate = await listen("update://available", () => {
      updateMessage.value = "发现可用更新，可前往软件更新查看";
    });
    unlistenNavigate = await listen<string>("app://navigate", (event) => {
      if (sections.some((item) => item.id === event.payload)) {
        section.value = event.payload as Section;
      }
    });
    unlistenToast = await listen<string>("app://toast", (event) => {
      notify(event.payload);
    });
    unlistenHistory = await listen("history://changed", () => {
      refreshHistory().catch(() => undefined);
    });
  }
});

onBeforeUnmount(() => {
  window.clearTimeout(toastTimer);
  unlistenUpdate?.();
  unlistenNavigate?.();
  unlistenToast?.();
  unlistenHistory?.();
});
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <AppIcon :size="38" />
        <div>
          <strong>S-Tools</strong>
          <span>轻巧的桌面工具箱</span>
        </div>
      </div>

      <nav class="nav">
        <button
          v-for="item in visibleSections"
          :key="item.id"
          :class="{ active: section === item.id }"
          @click="section = item.id"
        >
          <UiIcon :name="item.icon" />
          <span>{{ item.label }}</span>
          <span v-if="item.id === 'translation' && enabledProviders.length" class="nav-count">
            {{ enabledProviders.length }}
          </span>
        </button>
      </nav>

      <div class="sidebar-foot">
        <div class="status-dot"></div>
        <div>
          <strong>本地优先</strong>
          <span>不采集使用数据</span>
        </div>
      </div>
    </aside>

    <main class="content">
      <header class="topbar">
        <div>
          <span class="eyebrow">S-TOOLS · {{ data?.version || "1.0.0" }}</span>
          <h1>{{ sections.find((item) => item.id === section)?.label }}</h1>
        </div>
        <div class="top-actions">
          <button v-if="settingsForm.screenshotEnabled" class="secondary-button" @click="startCapture">
            <UiIcon name="screenshot" />
            截图
            <kbd>{{ formatHotkey(settingsForm.hotkeys.screenshot, isMac) }}</kbd>
          </button>
          <button v-if="settingsForm.translationEnabled" class="primary-button" @click="startTranslation">
            <UiIcon name="translate" />
            划词翻译
            <kbd>{{ formatHotkey(settingsForm.hotkeys.translate, isMac) }}</kbd>
          </button>
        </div>
      </header>

      <div v-if="loading" class="center-state">
        <span class="spinner"></span>
        <span>正在读取本地配置…</span>
      </div>
      <div v-else-if="error" class="center-state error-state">
        <UiIcon name="alert" :size="28" />
        <strong>应用初始化失败</strong>
        <span>{{ error }}</span>
        <button class="secondary-button" @click="load">重试</button>
      </div>

      <div v-else-if="section === 'home'" class="workspace">
        <section class="hero">
          <div class="hero-copy">
            <span class="hero-kicker"><UiIcon name="spark" :size="15" /> 即用即走，不打断思路</span>
            <h2>选中、按下、完成。</h2>
            <p>
              S-Tools 把划词翻译与截屏标注放在同一个轻量入口里。快捷键触发，结束即回到手头工作。
            </p>
            <div class="hero-actions">
              <button v-if="settingsForm.translationEnabled" class="primary-button" @click="startTranslation">
                <UiIcon name="translate" />
                翻译当前选中文本
              </button>
              <button v-if="settingsForm.screenshotEnabled" class="secondary-button" @click="startCapture">
                <UiIcon name="screenshot" />
                开始截图
              </button>
              <span v-if="!settingsForm.translationEnabled && !settingsForm.screenshotEnabled" class="hero-off-hint">
                在下方开启需要使用的功能
              </span>
            </div>
          </div>
          <div class="hero-art" aria-hidden="true">
            <div class="floating-card translation-card">
              <span>TRANSLATE</span>
              <strong>把复杂留给自己，<br />把简单留给使用。</strong>
              <div class="mini-line"></div>
              <small>Keep complexity inside.</small>
            </div>
            <div class="floating-card capture-card">
              <div class="capture-dots"><i></i><i></i><i></i></div>
              <div class="capture-frame"></div>
              <span>区域截图 · {{ formatHotkey(settingsForm.hotkeys.screenshot, isMac) }}</span>
            </div>
          </div>
        </section>

        <section class="feature-switches">
          <article :class="['feature-switch-card', { enabled: settingsForm.translationEnabled }]">
            <span class="feature-switch-icon"><UiIcon name="translate" :size="21" /></span>
            <div class="feature-switch-copy">
              <div>
                <strong>划词翻译</strong>
                <span>{{ settingsForm.translationEnabled ? "已开启" : "默认关闭" }}</span>
              </div>
              <p>在任意应用中选中文本，通过快捷键打开翻译浮窗。</p>
            </div>
            <button
              type="button"
              :class="['feature-switch', { active: settingsForm.translationEnabled }]"
              :aria-label="settingsForm.translationEnabled ? '关闭划词翻译' : '开启划词翻译'"
              :disabled="featureSaving"
              @click="toggleFeature('translation')"
            >
              <span></span>
            </button>
          </article>
          <article :class="['feature-switch-card', { enabled: settingsForm.screenshotEnabled }]">
            <span class="feature-switch-icon"><UiIcon name="screenshot" :size="21" /></span>
            <div class="feature-switch-copy">
              <div>
                <strong>截图标注</strong>
                <span>{{ settingsForm.screenshotEnabled ? "已开启" : "默认关闭" }}</span>
              </div>
              <p>捕获当前屏幕，完成标注、复制和保存。</p>
            </div>
            <button
              type="button"
              :class="['feature-switch', { active: settingsForm.screenshotEnabled }]"
              :aria-label="settingsForm.screenshotEnabled ? '关闭截图标注' : '开启截图标注'"
              :disabled="featureSaving"
              @click="toggleFeature('screenshot')"
            >
              <span></span>
            </button>
          </article>
        </section>

        <section class="stat-grid">
          <article v-if="settingsForm.translationEnabled" class="stat-card">
            <span class="stat-icon accent"><UiIcon name="provider" /></span>
            <div>
              <strong>{{ enabledProviders.length }}</strong>
              <span>已启用翻译服务</span>
            </div>
            <button class="ghost-button" @click="section = 'translation'">管理</button>
          </article>
          <article class="stat-card">
            <span class="stat-icon blue"><UiIcon name="history" /></span>
            <div>
              <strong>{{
                (data?.translationHistory.length || 0) + (data?.screenshotHistory.length || 0)
              }}</strong>
              <span>历史记录</span>
            </div>
            <button class="ghost-button" @click="section = 'history'">查看</button>
          </article>
          <article v-if="settingsForm.screenshotEnabled" class="stat-card">
            <span class="stat-icon green"><UiIcon name="image" /></span>
            <div>
              <strong>{{ data?.screenshotHistory.length || 0 }}</strong>
              <span>截图记录</span>
            </div>
            <button class="ghost-button" @click="section = 'history'; historyTab = 'screenshot'">查看</button>
          </article>
        </section>

        <section class="permission-strip">
          <div class="permission-heading">
            <span class="stat-icon"><UiIcon name="shield" /></span>
            <div>
              <strong>系统权限</strong>
              <span>划词读取和屏幕捕获需要系统授权</span>
            </div>
          </div>
          <div class="permission-items">
            <button
              class="permission-item"
              @click="openPermission('accessibility')"
            >
              <span :class="['permission-status', { ok: data?.permissions.accessibility }]">
                <UiIcon :name="data?.permissions.accessibility ? 'check' : 'alert'" :size="14" />
              </span>
              <span>
                <strong>辅助功能</strong>
                <small>{{ data?.permissions.accessibility ? "已授权" : "需要授权" }}</small>
              </span>
              <UiIcon name="arrow" :size="15" />
            </button>
            <button
              class="permission-item"
              @click="openPermission('screenRecording')"
            >
              <span :class="['permission-status', { ok: data?.permissions.screenRecording }]">
                <UiIcon :name="data?.permissions.screenRecording ? 'check' : 'alert'" :size="14" />
              </span>
              <span>
                <strong>屏幕录制</strong>
                <small>{{ data?.permissions.screenRecording ? "已授权" : "需要授权" }}</small>
              </span>
              <UiIcon name="arrow" :size="15" />
            </button>
          </div>
        </section>

        <section v-if="settingsForm.translationEnabled" class="quick-guide">
          <div>
            <span class="eyebrow">QUICK START</span>
            <h3>三步开始使用</h3>
          </div>
          <div class="guide-step">
            <strong>01</strong>
            <span>在任意应用选中文本</span>
          </div>
          <div class="guide-step">
            <strong>02</strong>
            <span>按下划词翻译快捷键</span>
          </div>
          <div class="guide-step">
            <strong>03</strong>
            <span>对照结果并一键复制</span>
          </div>
        </section>
      </div>

      <div v-else-if="section === 'history'" class="workspace">
        <section class="page-toolbar">
          <div class="segmented">
            <button
              :class="{ active: historyTab === 'translation' }"
              @click="historyTab = 'translation'"
            >
              翻译记录
              <span>{{ data?.translationHistory.length || 0 }}</span>
            </button>
            <button
              :class="{ active: historyTab === 'screenshot' }"
              @click="historyTab = 'screenshot'"
            >
              截图记录
              <span>{{ data?.screenshotHistory.length || 0 }}</span>
            </button>
          </div>
          <button class="ghost-button" @click="refreshHistory">
            <UiIcon name="update" />
            刷新
          </button>
        </section>

        <div v-if="currentHistory.length" class="history-list">
          <article v-for="item in currentHistory" :key="`${item.kind}-${item.id}`" class="history-item">
            <div v-if="item.kind === 'screenshot' && item.path" class="history-thumb">
              <img :src="assetUrl(item.path)" alt="" />
            </div>
            <div v-else class="history-thumb translation-thumb">
              <span>A</span>
              <UiIcon name="arrow" :size="15" />
              <span>文</span>
            </div>
            <div class="history-copy">
              <div class="history-title-row">
                <strong>{{ truncate(item.title || "未命名记录", 70) }}</strong>
                <span>{{ formatDate(item.createdAt) }}</span>
              </div>
              <p>{{ truncate(item.subtitle || (item.kind === "screenshot" ? "截图文件" : ""), 130) }}</p>
              <span v-if="item.providerId" class="badge">{{ item.providerId }}</span>
            </div>
            <div class="history-actions">
              <button class="ghost-button" title="复制" @click="copyHistory(item)">
                <UiIcon name="copy" />
              </button>
              <button class="ghost-button danger-text" title="删除" @click="deleteHistory(item)">
                <UiIcon name="trash" />
              </button>
            </div>
          </article>
        </div>
        <div v-else class="empty-state">
          <span class="empty-icon"><UiIcon name="history" :size="28" /></span>
          <strong>还没有记录</strong>
          <span>完成一次{{ historyTab === "translation" ? "翻译" : "截图" }}后，记录会出现在这里。</span>
        </div>
      </div>

      <div v-else-if="section === 'translation'" class="workspace translation-workspace">
        <section class="section-intro">
          <div>
            <span class="eyebrow">PROVIDERS</span>
            <h2>划词翻译</h2>
            <p>按需启用，可同时对照多个服务的结果。密钥仅保存在系统凭据库。</p>
          </div>
          <span class="badge accent">{{ enabledProviders.length }} 个服务已启用</span>
        </section>

        <div class="tool-settings-grid">
          <section class="settings-section">
            <div class="settings-heading">
              <span class="stat-icon"><UiIcon name="keyboard" /></span>
              <div>
                <h2>翻译快捷键</h2>
                <p>选中文本后按此组合键打开翻译浮窗。</p>
              </div>
            </div>
            <HotkeyRecorder
              v-model="settingsForm.hotkeys.translate"
              label="划词翻译快捷键"
              :conflict-value="settingsForm.hotkeys.screenshot"
            />
          </section>
          <section class="settings-section">
            <div class="settings-heading">
              <span class="stat-icon"><UiIcon name="language" /></span>
              <div>
                <h2>翻译偏好</h2>
                <p>“自动”会按选中文本是否含中文决定目标语言。</p>
              </div>
            </div>
            <label class="field">
              <span>默认目标语言</span>
              <select v-model="settingsForm.targetLang" class="select-input">
                <option value="auto">自动判断</option>
                <option value="zh-CN">简体中文</option>
                <option value="zh-TW">繁体中文</option>
                <option value="en">英语</option>
                <option value="ja">日语</option>
                <option value="ko">韩语</option>
              </select>
            </label>
          </section>
        </div>

        <div class="settings-save">
          <button class="primary-button" :disabled="settingsSaving" @click="saveSettings">
            <span v-if="settingsSaving" class="spinner"></span>
            {{ settingsSaving ? "保存中…" : "保存翻译设置" }}
          </button>
        </div>

        <div class="provider-grid">
          <article
            v-for="provider in data?.providers"
            :key="provider.id"
            :class="['provider-card', { enabled: provider.enabled, configured: provider.configured }]"
          >
            <div class="provider-card-head">
              <div class="provider-logo">{{ provider.name.slice(0, 1) }}</div>
              <button
                :class="['toggle', { active: provider.enabled }]"
                :aria-label="provider.enabled ? '停用' : '启用'"
                @click="toggleProvider(provider)"
              >
                <span></span>
              </button>
            </div>
            <div>
              <h3>{{ provider.name }}</h3>
              <p>{{ provider.description }}</p>
            </div>
            <div class="provider-card-foot">
              <span :class="['badge', { success: provider.configured }]">
                {{ provider.configured ? "已配置" : "待配置" }}
              </span>
              <button class="ghost-button" @click="openProvider(provider)">
                <UiIcon name="edit" :size="15" />
                配置
              </button>
            </div>
          </article>
        </div>
      </div>

      <div v-else-if="section === 'screenshot'" class="workspace screenshot-workspace">
        <section class="section-intro">
          <div>
            <span class="eyebrow">SCREENSHOT</span>
            <h2>截图标注</h2>
            <p>冻结当前屏幕，完成框选、标注、复制和保存。</p>
          </div>
          <button class="primary-button" @click="startCapture">
            <UiIcon name="screenshot" />
            开始截图
            <kbd>{{ formatHotkey(settingsForm.hotkeys.screenshot, isMac) }}</kbd>
          </button>
        </section>

        <div class="screenshot-feature-grid">
          <article class="screenshot-action-panel">
            <div class="screenshot-art">
              <span class="capture-window-dot"></span>
              <span class="capture-window-dot"></span>
              <span class="capture-window-dot"></span>
              <div class="screenshot-selection">
                <i></i><i></i><i></i><i></i>
              </div>
            </div>
            <div>
              <span class="eyebrow">READY TO CAPTURE</span>
              <h3>区域截图</h3>
              <p>支持矩形、椭圆、箭头、画笔、文字和马赛克标注。</p>
            </div>
            <button class="secondary-button" @click="startCapture">
              <UiIcon name="screenshot" />
              启动截图
            </button>
          </article>

          <article class="screenshot-config-panel">
            <div class="feature-panel-head">
              <span class="stat-icon green"><UiIcon name="image" /></span>
              <div>
                <strong>功能状态</strong>
                <span>关闭后隐藏截图导航并注销截图快捷键</span>
              </div>
            </div>
            <div class="settings-row">
              <span>
                <strong>截图标注</strong>
                <small>当前为{{ settingsForm.screenshotEnabled ? "开启" : "关闭" }}状态</small>
              </span>
              <button
                type="button"
                :class="['toggle', { active: settingsForm.screenshotEnabled }]"
                :aria-label="settingsForm.screenshotEnabled ? '关闭截图' : '开启截图'"
                :disabled="featureSaving"
                @click="toggleFeature('screenshot')"
              >
                <span></span>
              </button>
            </div>
            <div class="field">
              <span>截图快捷键</span>
              <HotkeyRecorder
                v-model="settingsForm.hotkeys.screenshot"
                label="区域截图快捷键"
                :conflict-value="settingsForm.hotkeys.translate"
              />
            </div>
            <div class="screenshot-summary">
              <div>
                <strong>{{ data?.screenshotHistory.length || 0 }}</strong>
                <span>截图历史</span>
              </div>
              <div>
                <strong>{{ settingsForm.screenshotEnabled ? "已开启" : "已关闭" }}</strong>
                <span>功能状态</span>
              </div>
            </div>
          </article>
        </div>

        <div class="settings-save">
          <button class="primary-button" :disabled="settingsSaving" @click="saveSettings">
            <span v-if="settingsSaving" class="spinner"></span>
            {{ settingsSaving ? "保存中…" : "保存截图设置" }}
          </button>
        </div>

        <section class="recent-captures">
          <div class="recent-captures-head">
            <div>
              <span class="eyebrow">RECENT CAPTURES</span>
              <h3>最近截图</h3>
            </div>
            <button
              class="ghost-button"
              @click="section = 'history'; historyTab = 'screenshot'"
            >
              查看全部
              <UiIcon name="arrow" :size="14" />
            </button>
          </div>
          <div v-if="recentScreenshots.length" class="recent-capture-grid">
            <article v-for="item in recentScreenshots" :key="item.id" class="recent-capture-card">
              <button class="recent-thumb" @click="copyHistory(item)">
                <img v-if="item.path" :src="assetUrl(item.path)" alt="" />
              </button>
              <div>
                <strong>{{ formatDate(item.createdAt) }}</strong>
                <span>{{ item.subtitle }}</span>
              </div>
              <button class="ghost-button danger-text" title="删除" @click="deleteHistory(item)">
                <UiIcon name="trash" :size="14" />
              </button>
            </article>
          </div>
          <div v-else class="screenshot-empty">
            <UiIcon name="image" :size="24" />
            <span>还没有截图，开启功能后按快捷键试试。</span>
          </div>
        </section>
      </div>

      <div v-else-if="section === 'settings'" class="workspace settings-workspace">
        <section class="settings-section">
          <div class="settings-heading">
            <span class="stat-icon"><UiIcon name="settings" /></span>
            <div>
              <h2>应用行为</h2>
              <p>关闭主窗口后应用仍驻留托盘。</p>
            </div>
          </div>
          <div class="settings-rows">
            <label class="settings-row">
              <span>
                <strong>开机自动启动</strong>
                <small>登录系统后在托盘常驻</small>
              </span>
              <input v-model="settingsForm.launchAtLogin" type="checkbox" class="switch-input" />
            </label>
            <label class="settings-row">
              <span>
                <strong>历史保留数量</strong>
                <small>翻译与截图各自保留的记录上限</small>
              </span>
              <input
                v-model.number="settingsForm.historyLimit"
                class="text-input compact-input"
                type="number"
                min="20"
                max="1000"
              />
            </label>
            <label class="settings-row">
              <span>
                <strong>外观</strong>
                <small>跟随系统或固定主题</small>
              </span>
              <select v-model="settingsForm.theme" class="select-input compact-input">
                <option value="system">跟随系统</option>
                <option value="dark">深色</option>
                <option value="light">浅色</option>
              </select>
            </label>
          </div>
        </section>

        <div class="settings-save">
          <button class="primary-button" :disabled="settingsSaving" @click="saveSettings">
            <span v-if="settingsSaving" class="spinner"></span>
            {{ settingsSaving ? "保存中…" : "保存设置" }}
          </button>
        </div>
      </div>

      <div v-else class="workspace updates-workspace">
        <section class="update-hero">
          <div class="update-mark"><AppIcon :size="72" /></div>
          <div>
            <span class="eyebrow">S-TOOLS DESKTOP</span>
            <h2>版本 {{ data?.version || "1.0.0" }}</h2>
            <p>更新清单来自 GitHub Release，下载后会校验 SHA256 再安装。</p>
          </div>
          <button class="primary-button" :disabled="updateChecking" @click="checkUpdate">
            <span v-if="updateChecking" class="spinner"></span>
            {{ updateChecking ? "检查中…" : "检查更新" }}
          </button>
        </section>

        <section v-if="updateInfo" class="update-card">
          <div class="update-card-head">
            <div>
              <span class="badge accent">新版本</span>
              <h3>v{{ updateInfo.version }}</h3>
            </div>
            <span>{{ updateInfo.selfUpdate ? "自动安装" : "手动安装" }}</span>
          </div>
          <p class="update-notes">{{ updateInfo.notes || "本次更新暂无说明。" }}</p>
          <div v-if="updateInstalling" class="progress-track">
            <span :style="{ width: `${updateProgress}%` }"></span>
          </div>
          <button class="primary-button" :disabled="updateInstalling" @click="installUpdate">
            {{ updateInstalling ? `下载中 ${updateProgress}%` : updateInfo.selfUpdate ? "下载并安装" : "下载安装包" }}
          </button>
        </section>

        <div v-if="updateMessage" class="notice-box success">
          <UiIcon name="check" />
          {{ updateMessage }}
        </div>
        <div v-if="updateError" class="notice-box danger">
          <UiIcon name="alert" />
          {{ updateError }}
        </div>

        <section class="release-notes">
          <h3>发布规则</h3>
          <p>
            macOS 提供 DMG 与可自更新 app.zip，Windows 提供 NSIS 安装包。当前版本使用 ad-hoc
            签名，系统可能要求首次手动放行；升级后可能需要重新授予辅助功能或屏幕录制权限。
          </p>
        </section>
      </div>
    </main>

    <div v-if="editing" class="modal-backdrop" @click.self="editing = null">
      <form class="modal" @submit.prevent="saveProvider">
        <div class="modal-head">
          <div>
            <span class="eyebrow">CONFIGURE</span>
            <h2>{{ editing.name }}</h2>
          </div>
          <button type="button" class="ghost-button" @click="editing = null">
            <UiIcon name="close" />
          </button>
        </div>

        <div class="settings-rows modal-fields">
          <label v-if="editing.kind === 'baidu'" class="field">
            <span>App ID</span>
            <input v-model="credentials.appId" class="text-input" autocomplete="off" />
          </label>
          <label v-if="editing.kind === 'baidu'" class="field">
            <span>App 密钥</span>
            <input v-model="credentials.secret" class="text-input" type="password" autocomplete="off" />
          </label>
          <label v-if="editing.kind === 'google'" class="field">
            <span>Google API Key</span>
            <input v-model="credentials.apiKey" class="text-input" type="password" autocomplete="off" />
          </label>
          <template v-if="editing.kind === 'tencent'">
            <label class="field">
              <span>Secret ID</span>
              <input v-model="credentials.secretId" class="text-input" autocomplete="off" />
            </label>
            <label class="field">
              <span>Secret Key</span>
              <input v-model="credentials.secretKey" class="text-input" type="password" autocomplete="off" />
            </label>
          </template>
          <label v-if="['deepseek', 'openai-compatible'].includes(editing.kind)" class="field">
            <span>API Key</span>
            <input v-model="credentials.apiKey" class="text-input" type="password" autocomplete="off" />
          </label>
          <label v-if="['deepseek', 'openai-compatible'].includes(editing.kind)" class="field">
            <span>Base URL</span>
            <input
              v-model="editing.baseUrl"
              class="text-input"
              placeholder="http://192.168.1.20:8000/v1"
            />
            <p v-if="insecureBaseUrl" class="http-warning">
              <UiIcon name="alert" :size="14" />
              HTTP 会明文传输 API Key，请仅用于可信内网或本机服务。
            </p>
          </label>
          <label v-if="['deepseek', 'openai-compatible'].includes(editing.kind)" class="field">
            <span>模型</span>
            <input v-model="editing.model" class="text-input" placeholder="deepseek-chat" />
          </label>
        </div>

        <p class="modal-hint">
          已保存的密钥不会回显。留空会保留原密钥；填写新值会覆盖系统凭据库中的旧密钥。
        </p>

        <div class="modal-actions">
          <label class="inline-check">
            <input v-model="editing.enabled" type="checkbox" />
            保存后启用此服务
          </label>
          <div>
            <button type="button" class="ghost-button" @click="editing = null">取消</button>
            <button type="button" class="secondary-button" :disabled="providerSaving" @click="testProvider">
              {{ providerSaving ? "检测中…" : "保存并测试" }}
            </button>
            <button class="primary-button" :disabled="providerSaving">
              {{ providerSaving ? "保存中…" : "保存配置" }}
            </button>
          </div>
        </div>
      </form>
    </div>

    <Transition name="toast">
      <div v-if="toast" class="toast">{{ toast }}</div>
    </Transition>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: 232px minmax(0, 1fr);
  width: 100%;
  height: 100%;
  background:
    radial-gradient(circle at 72% -12%, rgba(247, 147, 30, 0.08), transparent 30%),
    var(--bg);
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 30px;
  min-width: 0;
  padding: 24px 18px 20px;
  border-right: 1px solid var(--line);
  background: color-mix(in srgb, var(--bg-elevated) 92%, transparent);
}

.brand {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 4px 8px;
}

.brand strong,
.brand span {
  display: block;
}

.brand strong {
  font-size: 16px;
  letter-spacing: -0.02em;
}

.brand span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 11px;
}

.nav {
  display: grid;
  gap: 6px;
}

.nav button {
  display: flex;
  align-items: center;
  gap: 11px;
  width: 100%;
  height: 42px;
  padding: 0 12px;
  border: 0;
  border-radius: 11px;
  background: transparent;
  color: var(--text-soft);
  text-align: left;
  cursor: pointer;
  transition:
    background 160ms ease,
    color 160ms ease;
}

.nav button:hover {
  background: var(--bg-soft);
  color: var(--text);
}

.nav button.active {
  background: var(--accent-soft);
  color: var(--accent-strong);
  font-weight: 650;
}

.nav-count {
  min-width: 20px;
  margin-left: auto;
  padding: 2px 6px;
  border-radius: 999px;
  background: rgba(247, 147, 30, 0.18);
  color: var(--accent-strong);
  font-size: 10px;
  text-align: center;
}

.sidebar-foot {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: auto;
  padding: 14px 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--bg-soft);
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--success);
  box-shadow: 0 0 0 4px rgba(82, 211, 154, 0.1);
}

.sidebar-foot strong,
.sidebar-foot span {
  display: block;
}

.sidebar-foot strong {
  font-size: 11px;
}

.sidebar-foot span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 10px;
}

.content {
  min-width: 0;
  height: 100%;
  overflow-y: auto;
  padding: 26px 32px 42px;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  max-width: 1120px;
  margin: 0 auto 26px;
}

.topbar h1 {
  margin: 4px 0 0;
  font-size: 25px;
  font-weight: 720;
  letter-spacing: -0.035em;
}

.eyebrow {
  color: var(--text-muted);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.13em;
}

.top-actions {
  display: flex;
  gap: 10px;
}

kbd {
  margin-left: 5px;
  padding: 2px 6px;
  border: 1px solid color-mix(in srgb, currentColor 22%, transparent);
  border-radius: 5px;
  background: color-mix(in srgb, currentColor 9%, transparent);
  font-family: inherit;
  font-size: 10px;
}

.workspace {
  max-width: 1120px;
  margin: 0 auto;
  animation: fade-up 280ms ease both;
}

.hero {
  display: grid;
  grid-template-columns: minmax(0, 1.05fr) minmax(320px, 0.95fr);
  min-height: 310px;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: var(--radius-lg);
  background:
    linear-gradient(125deg, rgba(247, 147, 30, 0.09), transparent 45%),
    var(--bg-elevated);
}

.hero-copy {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: center;
  padding: 40px;
}

.hero-kicker {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--accent-strong);
  font-size: 11px;
  font-weight: 650;
}

.hero h2 {
  max-width: 560px;
  margin: 15px 0 12px;
  font-size: clamp(32px, 4vw, 46px);
  line-height: 1.08;
  letter-spacing: -0.055em;
}

.hero p {
  max-width: 500px;
  margin: 0;
  color: var(--text-soft);
  font-size: 14px;
  line-height: 1.75;
}

.hero-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 28px;
}

.hero-off-hint {
  color: var(--text-muted);
  font-size: 11px;
}

.hero-art {
  position: relative;
  min-height: 300px;
  overflow: hidden;
  background:
    linear-gradient(150deg, rgba(255, 255, 255, 0.03), transparent),
    repeating-linear-gradient(
      90deg,
      transparent 0,
      transparent 35px,
      rgba(255, 255, 255, 0.025) 36px
    );
}

.hero-art::after {
  position: absolute;
  inset: 50% auto auto 50%;
  width: 280px;
  height: 280px;
  border: 1px solid rgba(247, 147, 30, 0.18);
  border-radius: 50%;
  content: "";
  transform: translate(-50%, -50%);
}

.floating-card {
  position: absolute;
  z-index: 1;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 18px;
  box-shadow: 0 28px 60px rgba(0, 0, 0, 0.28);
}

.translation-card {
  top: 44px;
  right: 34px;
  width: 290px;
  padding: 22px;
  background: #f2eee5;
  color: #17181b;
  transform: rotate(2.5deg);
}

.translation-card > span {
  color: #8a857b;
  font-size: 9px;
  font-weight: 800;
  letter-spacing: 0.13em;
}

.translation-card strong {
  display: block;
  margin: 16px 0;
  font-size: 19px;
  line-height: 1.45;
  letter-spacing: -0.035em;
}

.translation-card small {
  color: #8a857b;
  font-size: 10px;
}

.mini-line {
  width: 28px;
  height: 3px;
  margin-bottom: 18px;
  border-radius: 999px;
  background: #f7931e;
}

.capture-card {
  right: 156px;
  bottom: 29px;
  width: 220px;
  padding: 14px;
  background: #222329;
  transform: rotate(-4deg);
}

.capture-card > span {
  color: #a2a19c;
  font-size: 9px;
  font-weight: 650;
}

.capture-dots {
  display: flex;
  gap: 4px;
  margin-bottom: 10px;
}

.capture-dots i {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: #55565d;
}

.capture-dots i:first-child {
  background: #f7931e;
}

.capture-frame {
  height: 84px;
  margin-bottom: 10px;
  border: 1px dashed rgba(247, 147, 30, 0.65);
  border-radius: 8px;
  background:
    linear-gradient(135deg, rgba(247, 147, 30, 0.12), transparent),
    repeating-linear-gradient(
      45deg,
      rgba(255, 255, 255, 0.025) 0,
      rgba(255, 255, 255, 0.025) 5px,
      transparent 5px,
      transparent 10px
    );
}

.feature-switches {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  margin-top: 16px;
}

.feature-switch-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 18px;
  border: 1px solid var(--line);
  border-radius: 17px;
  background: var(--bg-elevated);
  transition:
    border-color 160ms ease,
    background 160ms ease;
}

.feature-switch-card.enabled {
  border-color: rgba(247, 147, 30, 0.34);
  background:
    linear-gradient(135deg, rgba(247, 147, 30, 0.07), transparent 55%),
    var(--bg-elevated);
}

.feature-switch-icon {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  border-radius: 12px;
  background: var(--bg-soft);
  color: var(--text-soft);
}

.feature-switch-card.enabled .feature-switch-icon {
  background: var(--accent-soft);
  color: var(--accent-strong);
}

.feature-switch-copy {
  min-width: 0;
  flex: 1;
}

.feature-switch-copy > div {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.feature-switch-copy strong {
  font-size: 13px;
}

.feature-switch-copy span {
  color: var(--text-muted);
  font-size: 9px;
}

.feature-switch-copy p {
  margin: 5px 0 0;
  color: var(--text-muted);
  font-size: 10px;
  line-height: 1.55;
}

.feature-switch {
  position: relative;
  flex: 0 0 auto;
  width: 40px;
  height: 23px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--bg-soft);
  cursor: pointer;
  transition: background 160ms ease;
}

.feature-switch span {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 17px;
  height: 17px;
  border-radius: 50%;
  background: var(--text-muted);
  transition:
    transform 160ms ease,
    background 160ms ease;
}

.feature-switch.active {
  background: var(--accent);
}

.feature-switch.active span {
  background: #1d1408;
  transform: translateX(17px);
}

.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 14px;
  margin-top: 16px;
}

.stat-card {
  display: flex;
  align-items: center;
  gap: 14px;
  min-width: 0;
  padding: 17px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--bg-elevated);
}

.stat-icon {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 11px;
  background: var(--bg-soft);
  color: var(--text-soft);
}

.stat-icon.accent {
  background: var(--accent-soft);
  color: var(--accent-strong);
}

.stat-icon.blue {
  background: rgba(92, 145, 255, 0.12);
  color: #87a9ff;
}

.stat-icon.green {
  background: rgba(82, 211, 154, 0.11);
  color: var(--success);
}

.stat-card > div {
  min-width: 0;
}

.stat-card strong,
.stat-card span {
  display: block;
}

.stat-card strong {
  font-size: 20px;
  letter-spacing: -0.04em;
}

.stat-card span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 11px;
}

.stat-card .ghost-button {
  min-height: 30px;
  margin-left: auto;
  padding: 0 9px;
  font-size: 11px;
}

.permission-strip {
  display: grid;
  grid-template-columns: minmax(220px, 0.8fr) minmax(0, 1.2fr);
  gap: 20px;
  align-items: center;
  margin-top: 16px;
  padding: 18px 20px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--bg-elevated);
}

.permission-heading {
  display: flex;
  align-items: center;
  gap: 12px;
}

.permission-heading strong,
.permission-heading span {
  display: block;
}

.permission-heading strong {
  font-size: 13px;
}

.permission-heading span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 11px;
}

.permission-items {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.permission-item {
  display: grid;
  grid-template-columns: auto 1fr auto;
  gap: 10px;
  align-items: center;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--bg);
  color: var(--text);
  text-align: left;
  cursor: pointer;
}

.permission-item:hover {
  border-color: var(--line-strong);
}

.permission-item strong,
.permission-item small {
  display: block;
}

.permission-item strong {
  font-size: 11px;
}

.permission-item small {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: 10px;
}

.permission-status {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: rgba(255, 107, 104, 0.12);
  color: var(--danger);
}

.permission-status.ok {
  background: rgba(82, 211, 154, 0.12);
  color: var(--success);
}

.quick-guide {
  display: grid;
  grid-template-columns: 1.2fr repeat(3, 1fr);
  gap: 14px;
  align-items: center;
  margin-top: 16px;
  padding: 18px 20px;
  border: 1px dashed var(--line-strong);
  border-radius: 16px;
}

.quick-guide h3 {
  margin: 4px 0 0;
  font-size: 14px;
}

.guide-step {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--text-soft);
  font-size: 11px;
}

.guide-step strong {
  color: var(--accent-strong);
  font-size: 10px;
}

.page-toolbar,
.section-intro {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  margin-bottom: 18px;
}

.section-intro h2,
.settings-heading h2 {
  margin: 4px 0 0;
  font-size: 25px;
  letter-spacing: -0.04em;
}

.section-intro p,
.settings-heading p {
  margin: 7px 0 0;
  color: var(--text-soft);
  font-size: 12px;
  line-height: 1.65;
}

.segmented {
  display: flex;
  gap: 4px;
  padding: 4px;
  border-radius: 11px;
  background: var(--bg-elevated);
}

.segmented button {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 34px;
  padding: 0 13px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-soft);
  cursor: pointer;
}

.segmented button.active {
  background: var(--bg-soft);
  color: var(--text);
  font-weight: 650;
}

.segmented span {
  color: var(--text-muted);
  font-size: 10px;
}

.history-list {
  display: grid;
  gap: 10px;
}

.history-item {
  display: flex;
  align-items: center;
  gap: 15px;
  padding: 13px;
  border: 1px solid var(--line);
  border-radius: 15px;
  background: var(--bg-elevated);
}

.history-thumb {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 74px;
  height: 52px;
  overflow: hidden;
  border-radius: 9px;
  background: var(--bg-soft);
}

.history-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.translation-thumb {
  gap: 7px;
  background: linear-gradient(135deg, var(--accent-soft), var(--bg-soft));
  color: var(--accent-strong);
  font-size: 13px;
  font-weight: 700;
}

.history-copy {
  min-width: 0;
  flex: 1;
}

.history-title-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 18px;
}

.history-title-row strong {
  overflow: hidden;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-title-row span {
  flex: 0 0 auto;
  color: var(--text-muted);
  font-size: 10px;
}

.history-copy p {
  margin: 6px 0;
  overflow: hidden;
  color: var(--text-soft);
  font-size: 11px;
  line-height: 1.5;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.history-actions {
  display: flex;
  gap: 2px;
}

.history-actions .ghost-button {
  width: 32px;
  min-height: 32px;
  padding: 0;
}

.danger-text {
  color: var(--danger);
}

.empty-state,
.center-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-height: 340px;
  color: var(--text-muted);
  text-align: center;
}

.empty-state {
  min-height: 380px;
  border: 1px dashed var(--line-strong);
  border-radius: 18px;
}

.empty-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 62px;
  height: 62px;
  margin-bottom: 16px;
  border-radius: 18px;
  background: var(--bg-soft);
}

.empty-state strong {
  color: var(--text);
  font-size: 14px;
}

.empty-state > span:last-child {
  margin-top: 6px;
  font-size: 11px;
}

.center-state {
  gap: 12px;
  min-height: 480px;
  font-size: 12px;
}

.error-state {
  color: var(--text-soft);
}

.provider-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
}

.provider-card {
  display: flex;
  flex-direction: column;
  min-height: 210px;
  padding: 18px;
  border: 1px solid var(--line);
  border-radius: 17px;
  background: var(--bg-elevated);
  transition:
    border-color 180ms ease,
    transform 180ms ease;
}

.provider-card:hover {
  transform: translateY(-2px);
}

.provider-card.enabled {
  border-color: rgba(247, 147, 30, 0.35);
}

.provider-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.provider-logo {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: 11px;
  background: var(--bg-soft);
  color: var(--accent-strong);
  font-size: 15px;
  font-weight: 750;
}

.toggle {
  position: relative;
  width: 34px;
  height: 20px;
  padding: 0;
  border: 0;
  border-radius: 999px;
  background: var(--bg-soft);
  cursor: pointer;
  transition: background 160ms ease;
}

.toggle span {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--text-muted);
  transition:
    transform 160ms ease,
    background 160ms ease;
}

.toggle.active {
  background: var(--accent);
}

.toggle.active span {
  background: #1d1408;
  transform: translateX(14px);
}

.provider-card h3 {
  margin: 18px 0 6px;
  font-size: 14px;
}

.provider-card p {
  margin: 0;
  color: var(--text-soft);
  font-size: 11px;
  line-height: 1.65;
}

.provider-card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: auto;
  padding-top: 18px;
}

.provider-card-foot .ghost-button {
  min-height: 28px;
  padding: 0 7px;
  font-size: 10px;
}

.translation-workspace,
.screenshot-workspace {
  display: grid;
  gap: 16px;
}

.tool-settings-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.screenshot-feature-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(300px, 0.75fr);
  gap: 14px;
}

.screenshot-action-panel,
.screenshot-config-panel,
.recent-captures {
  border: 1px solid var(--line);
  border-radius: 18px;
  background: var(--bg-elevated);
}

.screenshot-action-panel {
  display: grid;
  grid-template-columns: minmax(180px, 0.8fr) minmax(0, 1fr) auto;
  gap: 20px;
  align-items: center;
  padding: 20px;
}

.screenshot-art {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: 4px;
  height: 132px;
  padding: 12px;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 13px;
  background:
    linear-gradient(135deg, rgba(247, 147, 30, 0.08), transparent 55%),
    var(--bg);
}

.capture-window-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-muted);
  opacity: 0.6;
}

.capture-window-dot:first-child {
  background: var(--accent);
  opacity: 1;
}

.screenshot-selection {
  position: absolute;
  top: 45px;
  right: 32px;
  bottom: 24px;
  left: 32px;
  border: 1px dashed rgba(247, 147, 30, 0.8);
  background: rgba(247, 147, 30, 0.06);
}

.screenshot-selection i {
  position: absolute;
  width: 6px;
  height: 6px;
  border-radius: 1px;
  background: var(--accent);
}

.screenshot-selection i:nth-child(1) {
  top: -3px;
  left: -3px;
}

.screenshot-selection i:nth-child(2) {
  top: -3px;
  right: -3px;
}

.screenshot-selection i:nth-child(3) {
  bottom: -3px;
  left: -3px;
}

.screenshot-selection i:nth-child(4) {
  right: -3px;
  bottom: -3px;
}

.screenshot-action-panel h3 {
  margin: 6px 0 7px;
  font-size: 18px;
  letter-spacing: -0.03em;
}

.screenshot-action-panel p {
  margin: 0;
  color: var(--text-soft);
  font-size: 11px;
  line-height: 1.65;
}

.screenshot-config-panel {
  display: grid;
  gap: 16px;
  padding: 20px;
}

.feature-panel-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.feature-panel-head strong,
.feature-panel-head span {
  display: block;
}

.feature-panel-head strong {
  font-size: 12px;
}

.feature-panel-head span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 10px;
}

.screenshot-summary {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.screenshot-summary > div {
  padding: 12px;
  border-radius: 11px;
  background: var(--bg);
}

.screenshot-summary strong,
.screenshot-summary span {
  display: block;
}

.screenshot-summary strong {
  font-size: 16px;
  letter-spacing: -0.03em;
}

.screenshot-summary span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 9px;
}

.recent-captures {
  padding: 20px;
}

.recent-captures-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 18px;
  margin-bottom: 15px;
}

.recent-captures-head h3 {
  margin: 4px 0 0;
  font-size: 18px;
}

.recent-captures-head .ghost-button {
  min-height: 30px;
  padding: 0 7px;
  font-size: 10px;
}

.recent-capture-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}

.recent-capture-card {
  display: grid;
  grid-template-columns: 58px minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
  padding: 9px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--bg);
}

.recent-thumb {
  width: 58px;
  height: 42px;
  padding: 0;
  overflow: hidden;
  border: 0;
  border-radius: 7px;
  background: var(--bg-soft);
  cursor: pointer;
}

.recent-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.recent-capture-card strong,
.recent-capture-card span {
  display: block;
}

.recent-capture-card strong {
  font-size: 10px;
}

.recent-capture-card span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 9px;
}

.recent-capture-card > .ghost-button {
  width: 28px;
  min-height: 28px;
  padding: 0;
}

.screenshot-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 9px;
  min-height: 120px;
  border: 1px dashed var(--line-strong);
  border-radius: 12px;
  color: var(--text-muted);
  font-size: 10px;
}

.settings-workspace {
  display: grid;
  gap: 14px;
}

.settings-section {
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 17px;
  background: var(--bg-elevated);
}

.settings-heading {
  display: flex;
  align-items: flex-start;
  gap: 13px;
  margin-bottom: 18px;
}

.settings-heading h2 {
  font-size: 16px;
}

.settings-fields {
  display: grid;
  gap: 14px;
}

.settings-fields.two-columns {
  grid-template-columns: 1fr 1fr;
}

.settings-rows {
  display: grid;
  gap: 2px;
}

.settings-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  min-height: 56px;
  padding: 10px 0;
  border-top: 1px solid var(--line);
}

.settings-row:first-child {
  border-top: 0;
}

.settings-row strong,
.settings-row small {
  display: block;
}

.settings-row strong {
  font-size: 12px;
}

.settings-row small {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 10px;
}

.compact-input {
  width: 160px;
}

.switch-input {
  width: 36px;
  height: 20px;
  accent-color: var(--accent);
}

.settings-save {
  display: flex;
  justify-content: flex-end;
}

.updates-workspace {
  display: grid;
  gap: 14px;
}

.update-hero,
.update-card,
.release-notes {
  border: 1px solid var(--line);
  border-radius: 19px;
  background: var(--bg-elevated);
}

.update-hero {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 24px;
}

.update-hero > div:nth-child(2) {
  flex: 1;
}

.update-hero h2 {
  margin: 5px 0 5px;
  font-size: 25px;
  letter-spacing: -0.04em;
}

.update-hero p,
.update-notes {
  margin: 0;
  color: var(--text-soft);
  font-size: 12px;
  line-height: 1.7;
}

.update-card {
  padding: 22px;
}

.update-card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
}

.update-card h3 {
  margin: 8px 0 0;
  font-size: 21px;
}

.update-card-head > span {
  color: var(--text-muted);
  font-size: 10px;
}

.update-notes {
  margin: 15px 0;
  padding: 14px;
  border-radius: 12px;
  background: var(--bg);
  white-space: pre-wrap;
}

.progress-track {
  height: 5px;
  margin-bottom: 16px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--bg-soft);
}

.progress-track span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
  transition: width 200ms ease;
}

.notice-box {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 13px 16px;
  border-radius: 12px;
  font-size: 11px;
}

.notice-box.success {
  background: rgba(82, 211, 154, 0.1);
  color: var(--success);
}

.notice-box.danger {
  background: rgba(255, 107, 104, 0.1);
  color: var(--danger);
}

.release-notes {
  padding: 22px;
}

.release-notes h3 {
  margin: 0 0 8px;
  font-size: 14px;
}

.release-notes p {
  margin: 0;
  color: var(--text-soft);
  font-size: 11px;
  line-height: 1.75;
}

.modal-backdrop {
  position: fixed;
  z-index: 50;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.56);
  backdrop-filter: blur(8px);
}

.modal {
  width: min(540px, 100%);
  max-height: calc(100vh - 48px);
  overflow-y: auto;
  padding: 24px;
  border: 1px solid var(--line-strong);
  border-radius: 20px;
  background: var(--bg-elevated);
  box-shadow: 0 32px 100px rgba(0, 0, 0, 0.5);
  animation: fade-up 200ms ease both;
}

.modal-head,
.modal-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
}

.modal-head h2 {
  margin: 5px 0 0;
  font-size: 21px;
}

.modal-head .ghost-button {
  width: 34px;
  min-height: 34px;
  padding: 0;
}

.modal-fields {
  margin-top: 20px;
}

.modal-fields .field {
  padding-top: 12px;
}

.modal-hint {
  margin: 18px 0;
  padding: 12px;
  border-radius: 10px;
  background: var(--bg);
  color: var(--text-muted);
  font-size: 10px;
  line-height: 1.65;
}

.http-warning {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin: 2px 0 0;
  padding: 9px 10px;
  border-radius: 9px;
  background: rgba(255, 107, 104, 0.09);
  color: var(--danger);
  font-size: 10px;
  line-height: 1.55;
}

.http-warning svg {
  flex: 0 0 auto;
  margin-top: 1px;
}

.modal-actions > div {
  display: flex;
  gap: 8px;
}

.inline-check {
  display: flex;
  align-items: center;
  gap: 7px;
  color: var(--text-soft);
  font-size: 11px;
}

.inline-check input {
  accent-color: var(--accent);
}

.toast {
  position: fixed;
  z-index: 100;
  bottom: 26px;
  left: 50%;
  padding: 11px 17px;
  border: 1px solid var(--line-strong);
  border-radius: 999px;
  background: #24252a;
  color: #f7f4ee;
  font-size: 11px;
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.35);
  transform: translateX(-50%);
}

.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 160ms ease,
    transform 160ms ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 8px);
}

@media (max-width: 980px) {
  .shell {
    grid-template-columns: 196px minmax(0, 1fr);
  }

  .content {
    padding: 24px;
  }

  .hero {
    grid-template-columns: 1fr;
  }

  .hero-art {
    display: none;
  }

  .provider-grid {
    grid-template-columns: 1fr 1fr;
  }

  .screenshot-feature-grid {
    grid-template-columns: 1fr;
  }

  .tool-settings-grid {
    grid-template-columns: 1fr;
  }

  .recent-capture-grid {
    grid-template-columns: 1fr 1fr;
  }

  .permission-strip {
    grid-template-columns: 1fr;
  }

  .quick-guide {
    grid-template-columns: 1fr 1fr;
  }
}

@media (max-width: 720px) {
  .shell {
    grid-template-columns: 1fr;
  }

  .sidebar {
    display: none;
  }

  .topbar {
    align-items: flex-start;
  }

  .top-actions {
    flex-direction: column;
  }

  .stat-grid,
  .provider-grid,
  .feature-switches,
  .recent-capture-grid,
  .settings-fields.two-columns {
    grid-template-columns: 1fr;
  }

  .screenshot-action-panel {
    grid-template-columns: 1fr;
  }

  .permission-items {
    grid-template-columns: 1fr;
  }

  .quick-guide {
    grid-template-columns: 1fr;
  }
}
</style>
