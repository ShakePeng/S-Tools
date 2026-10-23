<script setup lang="ts">
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import AppIcon from "../components/AppIcon.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import ProviderLogo from "../components/ProviderLogo.vue";
import UiIcon from "../components/UiIcon.vue";
import { truncate } from "../lib/format";
import { formatHotkey } from "../lib/hotkeys";
import { bootstrap, call, isTauri } from "../lib/ipc";
import { downloadProgressLabel, effectiveUpdateTotal, updatePercent } from "../lib/update";
import type {
  AppSettings,
  BootstrapData,
  ProviderInput,
  ProviderResult,
  ProviderView,
  SettingsInput,
  UpdateInfo,
  UpdateInstallResult,
  UpdateProgressEvent,
} from "../types";
import {
  isSectionVisible,
  normalizeSection,
  resolveInitialSection,
  resolveRequestedSection,
  visibleSectionItems,
  type Section,
} from "./navigation";

const SECTION_STORAGE_KEY = "s-tools.shell.section";
const rawRequestedSection = new URLSearchParams(window.location.search).get("section");
const rememberedSection = readRememberedSection();
const section = ref<Section>("overview");
const loading = ref(true);
const data = ref<BootstrapData | null>(null);
const error = ref("");
const toast = ref("");
const editing = ref<ProviderView | null>(null);
const credentials = reactive<Record<string, string>>({});
const providerSaving = ref(false);
const settingsSaving = ref(false);
const featureSaving = ref(false);
const updateChecking = ref(false);
const updateInfo = ref<UpdateInfo | null>(null);
type UpdatePhase = "idle" | "downloading" | "verifying" | "installing";
const updatePhase = ref<UpdatePhase>("idle");
const updateDownloaded = ref(0);
const updateTotal = ref<number | null>(null);
const updateCancelled = ref(false);
const updateCancelRequested = ref(false);
const updateCancelPending = ref(false);
const updateError = ref("");
const updateMessage = ref("");
const settingsForm = reactive<SettingsInput>({
  schemaVersion: 1,
  hotkeys: { translate: "Alt+D", screenshot: "F1" },
  translationEnabled: false,
  screenshotEnabled: false,
  targetLang: "auto",
  launchAtLogin: false,
  hasCompletedOnboarding: true,
});

let toastTimer: number | undefined;
let unlistenUpdate: (() => void) | undefined;
let unlistenNavigate: (() => void) | undefined;
let unlistenToast: (() => void) | undefined;
let unlistenCaptureError: (() => void) | undefined;

const enabledProviders = computed(
  () => data.value?.providers.filter((provider) => provider.enabled && provider.configured) || [],
);
const isMac = navigator.platform.toLowerCase().includes("mac");
const insecureBaseUrl = computed(
  () =>
    editing.value?.kind == "openai-compatible" &&
    editing.value?.baseUrl.trim().toLowerCase().startsWith("http://"),
);
const visibleSections = computed(() => visibleSectionItems(settingsForm));
const updateBusy = computed(() => updatePhase.value != "idle");
const updatePercentage = computed(() =>
  updatePercent(updateDownloaded.value, updateTotal.value),
);
const updateCanRetry = computed(
  () => !updateBusy.value && Boolean(updateError.value || updateCancelled.value),
);
const updateStatusText = computed(() => {
  if (updatePhase.value == "downloading") {
    const progress = downloadProgressLabel(updateDownloaded.value, updateTotal.value);
    return updatePercentage.value == null
      ? `正在下载 · ${progress}`
      : `正在下载 ${updatePercentage.value}% · ${progress}`;
  }
  if (updatePhase.value == "verifying") return "正在校验安装包…";
  if (updatePhase.value == "installing") return "正在启动安装，应用即将重启…";
  return "";
});
const updateButtonLabel = computed(() => {
  if (updatePhase.value == "downloading") return "下载中…";
  if (updatePhase.value == "verifying") return "校验中…";
  if (updatePhase.value == "installing") return "安装中…";
  if (updateCanRetry.value) return "重试下载";
  return updateInfo.value?.selfUpdate ? "下载并安装" : "下载安装包";
});

function readRememberedSection(): string | null {
  try {
    return window.localStorage.getItem(SECTION_STORAGE_KEY);
  } catch (reason) {
    console.warn("读取主页面 Tab 失败", reason);
    return null;
  }
}

function rememberSection(value: Section) {
  try {
    window.localStorage.setItem(SECTION_STORAGE_KEY, value);
  } catch (reason) {
    console.warn("保存主页面 Tab 失败", reason);
  }
}

watch(section, rememberSection, { flush: "sync" });

function notify(message: string) {
  toast.value = message;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toast.value = "";
  }, 2600);
}

function syncSettingsForm(settings: AppSettings) {
  Object.assign(settingsForm, JSON.parse(JSON.stringify(settings)));
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    data.value = await bootstrap();
    syncSettingsForm(data.value.settings);
    const initialSection = resolveInitialSection(
      rawRequestedSection,
      rememberedSection,
      data.value.settings,
    );
    section.value = initialSection;
    rememberSection(initialSection);
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
    if (!isSectionVisible(section.value, saved)) section.value = "overview";
    notify(`${kind == "translation" ? "划词翻译" : "截图"}已${saved[field] ? "开启" : "关闭"}`);
  } catch (reason) {
    settingsForm[field] = previous;
    notify(reason instanceof Error ? reason.message : String(reason));
  } finally {
    featureSaving.value = false;
  }
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

async function startCapture() {
  try {
    await call("start_capture");
  } catch (reason) {
    // 桌面端统一通过 app://toast 反馈，避免主窗口重复弹出两条提示。
    if (!isTauri()) notify(reason instanceof Error ? reason.message : String(reason));
  }
}

function openProvider(provider: ProviderView) {
  editing.value = provider;
  for (const key of Object.keys(credentials)) delete credentials[key];
  Object.assign(credentials, provider.credentials);
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
      useSystemProxy: editing.value.useSystemProxy,
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

async function openReleases() {
  try {
    await call("open_releases");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function openAppLog() {
  try {
    await call("open_app_log");
  } catch (reason) {
    notify(reason instanceof Error ? reason.message : String(reason));
  }
}

async function checkUpdate() {
  updateChecking.value = true;
  updatePhase.value = "idle";
  updateDownloaded.value = 0;
  updateTotal.value = null;
  updateCancelled.value = false;
  updateCancelRequested.value = false;
  updateCancelPending.value = false;
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
  const info = updateInfo.value;
  updatePhase.value = "downloading";
  updateDownloaded.value = 0;
  updateTotal.value = null;
  updateCancelled.value = false;
  updateCancelRequested.value = false;
  updateCancelPending.value = false;
  updateError.value = "";
  updateMessage.value = "";
  try {
    const channel = new Channel<UpdateProgressEvent>();
    channel.onmessage = (event) => {
      if (event.event == "Started") {
        updatePhase.value = "downloading";
        updateDownloaded.value = 0;
        updateTotal.value = effectiveUpdateTotal(event.contentLength, info.downloadSize);
      }
      if (event.event == "Progress") {
        updateDownloaded.value = event.downloaded || 0;
        updateTotal.value = effectiveUpdateTotal(event.contentLength, info.downloadSize);
      }
      if (event.event == "Verifying") updatePhase.value = "verifying";
      if (event.event == "Installing") updatePhase.value = "installing";
      if (event.event == "Finished" && updateTotal.value) {
        updateDownloaded.value = updateTotal.value;
      }
    };
    if (info.selfUpdate) {
      const result = await call<UpdateInstallResult>("install_update", {
        url: info.downloadUrl,
        fileName: info.fileName,
        sha256: info.sha256,
        onEvent: channel,
      });
      if (result.installed) {
        updatePhase.value = "installing";
        updateMessage.value = "正在安装并重启…";
        return;
      }
    }
    if (!info.manualDownloadUrl) {
      throw new Error("更新清单缺少手动下载地址");
    }
    updatePhase.value = "downloading";
    updateDownloaded.value = 0;
    updateTotal.value = info.manualDownloadSize ?? null;
    const path = await call<string>("download_update", {
      url: info.manualDownloadUrl,
      fileName: info.manualFileName || info.fileName,
      onEvent: channel,
    });
    await call("reveal_path", { path });
    updateMessage.value = "安装包已下载，请手动完成安装";
  } catch (reason) {
    const message = reason instanceof Error ? reason.message : String(reason);
    if (updateCancelRequested.value || message.includes("更新下载已取消")) {
      updateCancelled.value = true;
    } else {
      updateError.value = message;
    }
  } finally {
    updateCancelPending.value = false;
    if (updatePhase.value != "installing") updatePhase.value = "idle";
  }
}

async function cancelUpdateDownload() {
  if (updatePhase.value != "downloading" || updateCancelPending.value) return;
  updateCancelRequested.value = true;
  updateCancelPending.value = true;
  try {
    await call("cancel_update");
  } catch (reason) {
    updateCancelRequested.value = false;
    updateCancelPending.value = false;
    updateError.value = reason instanceof Error ? reason.message : String(reason);
  }
}

onMounted(async () => {
  await load();
  if (isTauri()) {
    unlistenUpdate = await listen("update://available", () => {
      updateMessage.value = "发现可用更新，可前往设置查看";
    });
    unlistenNavigate = await listen<string>("app://navigate", (event) => {
      const target = normalizeSection(event.payload);
      section.value =
        target && isSectionVisible(target, settingsForm) ? target : "overview";
    });
    unlistenToast = await listen<string>("app://toast", (event) => {
      notify(event.payload);
    });
    unlistenCaptureError = await listen<string>("app://capture-error", (event) => {
      section.value = isSectionVisible("screenshot", settingsForm) ? "screenshot" : "overview";
      nextTick(() => notify(event.payload));
    });
  }
});

onBeforeUnmount(() => {
  window.clearTimeout(toastTimer);
  unlistenUpdate?.();
  unlistenNavigate?.();
  unlistenToast?.();
  unlistenCaptureError?.();
});
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <AppIcon :size="38" />
        <div>
          <strong>
            S-Tools
            <span class="brand-version">v{{ data?.version || "1.0.10" }}</span>
          </strong>
          <span class="brand-subtitle">轻巧的桌面工具箱</span>
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

      <div v-else-if="section === 'overview'" class="workspace">
        <section class="section-intro">
          <div>
            <span class="eyebrow">PLUGINS</span>
            <h2>插件概览</h2>
            <p>按需启用插件。关闭后隐藏对应 Tab，并注销插件快捷键。</p>
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

      </div>

      <div v-else-if="section === 'translation'" class="workspace translation-workspace">
        <section class="section-intro">
          <div>
            <span class="eyebrow">PROVIDERS</span>
            <h2>划词翻译</h2>
            <p>按需启用，可同时对照多个服务的结果。密钥仅保存在本机应用配置中。</p>
          </div>
          <span class="badge accent">{{ enabledProviders.length }} 个服务已启用</span>
        </section>

        <section class="permission-strip plugin-permission-strip">
          <div class="permission-heading">
            <span class="stat-icon"><UiIcon name="shield" /></span>
            <div>
              <strong>辅助功能权限</strong>
              <span>用于读取当前应用中被选中的文本。</span>
            </div>
          </div>
          <div class="permission-items">
            <button
              v-if="isMac"
              class="permission-item"
              @click="openPermission('accessibility')"
            >
              <span :class="['permission-status', { ok: data?.permissions.accessibility }]">
                <UiIcon :name="data?.permissions.accessibility ? 'check' : 'alert'" :size="14" />
              </span>
              <span>
                <strong>{{ data?.permissions.accessibility ? "已授权" : "需要授权" }}</strong>
                <small>点击打开 macOS 系统设置</small>
              </span>
              <UiIcon name="arrow" :size="15" />
            </button>
            <div v-else class="permission-item static">
              <span class="permission-status ok"><UiIcon name="check" :size="14" /></span>
              <span>
                <strong>无需额外授权</strong>
                <small>Windows 通过模拟复制读取选中文本</small>
              </span>
            </div>
          </div>
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
              <select v-model="settingsForm.targetLang" class="select-input target-language-select">
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
            role="button"
            tabindex="0"
            :aria-label="`配置 ${provider.name}`"
            @click="openProvider(provider)"
            @keydown.enter="openProvider(provider)"
            @keydown.space.prevent="openProvider(provider)"
          >
            <div class="provider-card-head">
              <ProviderLogo :kind="provider.kind" />
              <button
                :class="['toggle', { active: provider.enabled }]"
                :aria-label="provider.enabled ? '停用' : '启用'"
                @click.stop="toggleProvider(provider)"
              >
                <span></span>
              </button>
            </div>
            <div>
              <h3>{{ provider.name }}</h3>
              <p>{{ provider.description }}</p>
            </div>
            <div class="provider-card-foot">
              <div class="provider-card-tags">
                <span :class="['badge', { success: provider.configured }]">
                  {{ provider.configured ? "已配置" : "待配置" }}
                </span>
                <span class="badge">{{ provider.useSystemProxy ? "系统代理" : "直连" }}</span>
              </div>
              <button class="ghost-button" @click.stop="openProvider(provider)">
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

        <section class="permission-strip plugin-permission-strip">
          <div class="permission-heading">
            <span class="stat-icon"><UiIcon name="shield" /></span>
            <div>
              <strong>屏幕录制权限</strong>
              <span>用于捕获当前显示器画面并进入标注编辑。</span>
            </div>
          </div>
          <div class="permission-items">
            <button
              v-if="isMac"
              class="permission-item"
              @click="openPermission('screenRecording')"
            >
              <span :class="['permission-status', { ok: data?.permissions.screenRecording }]">
                <UiIcon :name="data?.permissions.screenRecording ? 'check' : 'alert'" :size="14" />
              </span>
              <span>
                <strong>{{ data?.permissions.screenRecording ? "已授权" : "需要授权" }}</strong>
                <small>点击打开 macOS 系统设置</small>
              </span>
              <UiIcon name="arrow" :size="15" />
            </button>
            <div v-else class="permission-item static">
              <span class="permission-status ok"><UiIcon name="check" :size="14" /></span>
              <span>
                <strong>无需额外授权</strong>
                <small>Windows 可直接捕获屏幕</small>
              </span>
            </div>
          </div>
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
              <span class="stat-icon green"><UiIcon name="keyboard" /></span>
              <div>
                <strong>截图快捷键</strong>
                <span>按下快捷键即可进入截图模式</span>
              </div>
            </div>
            <div class="field">
              <HotkeyRecorder
                v-model="settingsForm.hotkeys.screenshot"
                label="区域截图快捷键"
                :conflict-value="settingsForm.hotkeys.translate"
              />
            </div>
          </article>
        </div>

        <div class="settings-save">
          <button class="primary-button" :disabled="settingsSaving" @click="saveSettings">
            <span v-if="settingsSaving" class="spinner"></span>
            {{ settingsSaving ? "保存中…" : "保存截图设置" }}
          </button>
        </div>

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
          </div>
        </section>

        <div class="settings-save">
          <button class="primary-button" :disabled="settingsSaving" @click="saveSettings">
            <span v-if="settingsSaving" class="spinner"></span>
            {{ settingsSaving ? "保存中…" : "保存设置" }}
          </button>
        </div>

        <section class="update-settings-section">
          <section class="update-hero">
            <div class="update-mark"><AppIcon :size="72" /></div>
            <div>
              <span class="eyebrow">S-TOOLS DESKTOP</span>
              <h2>版本 {{ data?.version || "1.0.10" }}</h2>
              <p>更新清单来自 GitHub Release，下载后会校验 SHA256 再安装。</p>
            </div>
            <div class="update-hero-actions">
              <button
                class="primary-button"
                :disabled="updateChecking || updateBusy"
                @click="checkUpdate"
              >
                <span v-if="updateChecking" class="spinner"></span>
                {{ updateChecking ? "检查中…" : "检查更新" }}
              </button>
              <button class="secondary-button" @click="openReleases">
                <UiIcon name="external" :size="15" />
                查看 Releases
              </button>
            </div>
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
            <div
              v-if="updatePhase === 'downloading' || updatePhase === 'verifying'"
              class="update-progress"
            >
              <div class="update-progress-meta">{{ updateStatusText }}</div>
              <div
                :class="['progress-track', { indeterminate: updatePercentage == null }]"
              >
                <i
                  v-if="updatePercentage != null"
                  :style="{ width: `${updatePercentage}%` }"
                ></i>
                <i v-else></i>
                <b v-if="updatePercentage != null">{{ updatePercentage }}%</b>
              </div>
            </div>
            <div v-else-if="updatePhase === 'installing'" class="update-stage">
              <span class="spinner"></span>
              {{ updateStatusText }}
            </div>
            <div class="update-card-actions">
              <button
                class="primary-button"
                :disabled="updateBusy"
                @click="installUpdate"
              >
                {{ updateButtonLabel }}
              </button>
              <button
                v-if="updatePhase === 'downloading'"
                class="ghost-button"
                :disabled="updateCancelPending"
                @click="cancelUpdateDownload"
              >
                {{ updateCancelPending ? "正在取消…" : "取消下载" }}
              </button>
            </div>
          </section>

          <div v-if="updateCancelled" class="notice-box neutral">
            <UiIcon name="close" />
            更新下载已取消，可点击“重试下载”重新开始。
          </div>
          <div v-if="updateMessage" class="notice-box success">
            <UiIcon name="check" />
            {{ updateMessage }}
          </div>
          <div v-if="updateError" class="notice-box danger">
            <UiIcon name="alert" />
            {{ updateError }}
          </div>

          <section class="settings-section">
            <div class="settings-heading">
              <span class="stat-icon"><UiIcon name="provider" /></span>
              <div>
                <h2>诊断</h2>
                <p>查看启动、截图和更新过程中的错误记录。</p>
              </div>
            </div>
            <button class="secondary-button" @click="openAppLog">
              <UiIcon name="external" :size="15" />
              打开日志
            </button>
          </section>
        </section>
      </div>
    </main>

    <div v-if="editing" class="modal-backdrop" @pointerdown.self="editing = null">
      <form class="modal" @pointerdown.stop @submit.prevent="saveProvider">
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
            <input v-model="credentials.secret" class="text-input" type="text" autocomplete="off" />
          </label>
          <label v-if="editing.kind === 'google'" class="field">
            <span>Google API Key</span>
            <input v-model="credentials.apiKey" class="text-input" type="text" autocomplete="off" />
          </label>
          <label v-if="editing.kind === 'openai-compatible'" class="field">
            <span>API Key</span>
            <input v-model="credentials.apiKey" class="text-input" type="text" autocomplete="off" />
          </label>
          <label v-if="editing.kind === 'openai-compatible'" class="field">
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
          <label v-if="editing.kind === 'openai-compatible'" class="field">
            <span>模型</span>
            <input v-model="editing.model" class="text-input" placeholder="gpt-4.1-mini" />
          </label>
        </div>

        <label class="proxy-setting">
          <span>
            <strong>走系统代理</strong>
            <small>默认关闭；仅海外服务无法直连时再按需开启。</small>
          </span>
          <input v-model="editing.useSystemProxy" type="checkbox" class="switch-input" />
        </label>

        <p class="modal-hint">
          空字段表示未配置；已保存的凭据会直接回显。填写新值会覆盖本机配置中的旧值。
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
    radial-gradient(circle at 72% -12%, rgba(245, 158, 11, 0.12), transparent 32%),
    linear-gradient(180deg, #292a2e 0%, #202124 100%);
}

.sidebar {
  display: flex;
  flex-direction: column;
  gap: 30px;
  min-width: 0;
  padding: 24px 18px 20px;
  border-right: 1px solid var(--line);
  background: rgba(42, 43, 47, 0.82);
  backdrop-filter: blur(20px) saturate(1.12);
}

.brand {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 4px 8px;
}

.brand strong {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: 16px;
  letter-spacing: -0.02em;
}

.brand-version {
  color: var(--text-muted);
  font-size: var(--font-xs);
  font-weight: 550;
  letter-spacing: 0;
}

.brand-subtitle {
  display: block;
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-sm);
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

.sidebar-foot {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: auto;
  padding: 14px 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.045);
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
  font-size: var(--font-sm);
}

.sidebar-foot span {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: var(--font-xs);
}

.content {
  min-width: 0;
  height: 100%;
  overflow-y: auto;
  padding: 32px 32px 42px;
}

.eyebrow {
  color: var(--text-muted);
  font-size: var(--font-xs);
  font-weight: 700;
  letter-spacing: 0.13em;
}

kbd {
  margin-left: 5px;
  padding: 2px 6px;
  border: 1px solid color-mix(in srgb, currentColor 22%, transparent);
  border-radius: 5px;
  background: color-mix(in srgb, currentColor 9%, transparent);
  font-family: inherit;
  font-size: var(--font-xs);
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
    linear-gradient(125deg, rgba(245, 158, 11, 0.09), transparent 45%),
    var(--bg-elevated);
  box-shadow: var(--shadow-card);
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
  font-size: var(--font-sm);
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
  font-size: var(--font-sm);
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
  border: 1px solid rgba(245, 158, 11, 0.18);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
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
  border: 1px dashed rgba(245, 158, 11, 0.65);
  border-radius: 8px;
  background:
    linear-gradient(135deg, rgba(245, 158, 11, 0.12), transparent),
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
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
  transition:
    border-color 160ms ease,
    background 160ms ease;
}

.feature-switch-card.enabled {
  border-color: rgba(245, 158, 11, 0.34);
  background:
    linear-gradient(135deg, rgba(245, 158, 11, 0.08), transparent 55%),
    rgba(48, 49, 54, 0.86);
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
  font-size: var(--font-lg);
}

.feature-switch-copy span {
  color: var(--text-muted);
  font-size: var(--font-xs);
}

.feature-switch-copy p {
  margin: 5px 0 0;
  color: var(--text-muted);
  font-size: var(--font-sm);
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
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
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

.stat-icon > svg {
  display: block;
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
  font-size: var(--font-sm);
}

.stat-card .ghost-button {
  min-height: 30px;
  margin-left: auto;
  padding: 0 9px;
  font-size: var(--font-sm);
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
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
}

.plugin-permission-strip {
  margin-bottom: 18px;
}

.permission-heading {
  display: flex;
  align-items: center;
  gap: 12px;
}

.permission-heading > div strong,
.permission-heading > div span {
  display: block;
}

.permission-heading > div strong {
  font-size: var(--font-lg);
}

.permission-heading > div span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-sm);
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
  background: rgba(255, 255, 255, 0.035);
  color: var(--text);
  text-align: left;
  cursor: pointer;
}

.permission-item:hover {
  border-color: var(--line-strong);
}

.permission-item.static {
  cursor: default;
}

.permission-item strong,
.permission-item small {
  display: block;
}

.permission-item strong {
  font-size: var(--font-sm);
}

.permission-item small {
  margin-top: 2px;
  color: var(--text-muted);
  font-size: var(--font-xs);
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
  font-size: var(--font-sm);
}

.guide-step strong {
  color: var(--accent-strong);
  font-size: var(--font-xs);
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
  font-size: var(--font-md);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-lg);
}

.empty-state > span:last-child {
  margin-top: 6px;
  font-size: var(--font-sm);
}

.center-state {
  gap: 12px;
  min-height: 480px;
  font-size: var(--font-md);
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
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
  cursor: pointer;
  transition:
    border-color 180ms ease,
    transform 180ms ease;
}

.provider-card:hover {
  transform: translateY(-2px);
}

.provider-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.provider-card.enabled {
  border-color: rgba(245, 158, 11, 0.35);
}

.provider-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
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
  font-size: var(--font-lg);
}

.provider-card p {
  margin: 0;
  color: var(--text-soft);
  font-size: var(--font-sm);
  line-height: 1.65;
}

.provider-card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: auto;
  padding-top: 18px;
}

.provider-card-tags {
  display: flex;
  align-items: center;
  gap: 6px;
}

.provider-card-foot .ghost-button {
  min-height: 28px;
  padding: 0 7px;
  font-size: var(--font-xs);
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
    linear-gradient(135deg, rgba(245, 158, 11, 0.08), transparent 55%),
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
  border: 1px dashed rgba(245, 158, 11, 0.8);
  background: rgba(245, 158, 11, 0.06);
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
  font-size: var(--font-sm);
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

.feature-panel-head > div strong,
.feature-panel-head > div span {
  display: block;
}

.feature-panel-head > div strong {
  font-size: var(--font-md);
}

.feature-panel-head > div span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
}

.recent-capture-card span {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-xs);
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
  font-size: var(--font-sm);
}

.settings-workspace {
  display: grid;
  gap: 14px;
}

.settings-section {
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 17px;
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
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

.target-language-select {
  background: var(--bg);
  box-shadow: none;
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
  font-size: var(--font-md);
}

.settings-row small {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-xs);
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

.update-settings-section {
  display: grid;
  gap: 18px;
  margin-top: 22px;
}

.update-hero,
.update-card {
  border: 1px solid var(--line);
  border-radius: 19px;
  background: rgba(48, 49, 54, 0.78);
  box-shadow: var(--shadow-card);
}

.update-hero {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 24px;
}

.update-hero-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 10px;
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
  font-size: var(--font-md);
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
  font-size: var(--font-xs);
}

.update-notes {
  margin: 15px 0;
  padding: 14px;
  border-radius: 12px;
  background: var(--bg);
  white-space: pre-wrap;
}

.update-progress {
  display: grid;
  gap: 8px;
  margin: 16px 0;
}

.update-progress-meta,
.update-stage {
  color: var(--text-soft);
  font-size: var(--font-sm);
}

.update-stage {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 16px 0;
}

.progress-track {
  position: relative;
  height: 20px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--bg-soft);
}

.progress-track > i {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--accent);
  transition: width 200ms ease;
}

.progress-track > b {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: #17130c;
  font-size: 10px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.progress-track.indeterminate > i {
  width: 34%;
  animation: update-progress-indeterminate 1.1s ease-in-out infinite;
}

.update-card-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 16px;
}

@keyframes update-progress-indeterminate {
  from {
    transform: translateX(-115%);
  }
  to {
    transform: translateX(330%);
  }
}

.notice-box {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 13px 16px;
  border-radius: 12px;
  font-size: var(--font-sm);
}

.notice-box.success {
  background: rgba(82, 211, 154, 0.1);
  color: var(--success);
}

.notice-box.danger {
  background: rgba(255, 107, 104, 0.1);
  color: var(--danger);
}

.notice-box.neutral {
  background: rgba(255, 255, 255, 0.055);
  color: var(--text-soft);
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
  background: #2a2b2f;
  box-shadow: 0 30px 90px rgba(0, 0, 0, 0.46);
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

.proxy-setting {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  margin-top: 18px;
  padding: 13px 14px;
  border: 1px solid var(--line);
  border-radius: 11px;
  background: rgba(255, 255, 255, 0.035);
}

.proxy-setting strong,
.proxy-setting small {
  display: block;
}

.proxy-setting strong {
  font-size: var(--font-md);
}

.proxy-setting small {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: var(--font-xs);
  line-height: 1.45;
}

.modal-hint {
  margin: 18px 0;
  padding: 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-muted);
  font-size: var(--font-xs);
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
  font-size: var(--font-xs);
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
  font-size: var(--font-sm);
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
  background: rgba(52, 53, 58, 0.96);
  color: var(--text);
  font-size: var(--font-sm);
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
