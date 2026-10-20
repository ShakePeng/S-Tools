<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import AppIcon from "../components/AppIcon.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import UiIcon from "../components/UiIcon.vue";
import { bootstrap, call } from "../lib/ipc";
import type { AppSettings, BootstrapData, PermissionStatus } from "../types";

const step = ref(0);
const data = ref<BootstrapData | null>(null);
const permissions = reactive<PermissionStatus>({
  accessibility: false,
  screenRecording: false,
});
const settings = reactive<AppSettings>({
  schemaVersion: 1,
  hotkeys: { translate: "Alt+D", screenshot: "F1" },
  translationEnabled: false,
  screenshotEnabled: false,
  targetLang: "auto",
  launchAtLogin: false,
  historyLimit: 200,
  theme: "dark",
  hasCompletedOnboarding: false,
});
const busy = ref(false);
const error = ref("");
const isMac = computed(() => navigator.platform.toLowerCase().includes("mac"));

async function refreshPermissions() {
  try {
    Object.assign(permissions, await call<PermissionStatus>("get_permission_status"));
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
}

async function openPermission(kind: "accessibility" | "screenRecording") {
  try {
    await call("open_permission_settings", { kind });
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
}

async function finish() {
  busy.value = true;
  try {
    settings.hasCompletedOnboarding = true;
    const saved = await call<AppSettings>("save_settings", { input: settings });
    Object.assign(settings, saved);
    await call("open_main_window", { section: "home" });
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  try {
    data.value = await bootstrap();
    Object.assign(settings, data.value.settings);
    Object.assign(permissions, data.value.permissions);
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
  }
});
</script>

<template>
  <main class="onboarding">
    <div class="onboarding-shell">
      <header class="onboarding-head">
        <div class="brand-lockup">
          <AppIcon :size="52" />
          <div>
            <span class="eyebrow">WELCOME TO</span>
            <h1>S-Tools</h1>
          </div>
        </div>
        <span class="step-label">0{{ step + 1 }} / 03</span>
      </header>

      <div v-if="step === 0" class="step-panel intro-step">
        <span class="step-icon"><UiIcon name="spark" :size="28" /></span>
        <h2>把常用工具，留在手边。</h2>
        <p>
          S-Tools 常驻托盘，用两个快捷键完成划词翻译和截图标注。无需切换窗口，也不会上传截图或使用数据。
        </p>
        <div class="feature-pair">
          <div>
            <UiIcon name="translate" :size="24" />
            <strong>划词翻译</strong>
            <span>多服务对照，选中文本后一步完成。</span>
          </div>
          <div>
            <UiIcon name="screenshot" :size="24" />
            <strong>截图标注</strong>
            <span>冻结画面、标注、复制和保存。</span>
          </div>
        </div>
      </div>

      <div v-else-if="step === 1" class="step-panel">
        <span class="step-icon"><UiIcon name="shield" :size="28" /></span>
        <h2>授予必要权限</h2>
        <p>
          划词读取需要辅助功能权限，截图需要屏幕录制权限。S-Tools 只在你主动触发时访问这些能力。
        </p>
        <div class="permission-list">
          <article>
            <div class="permission-copy">
              <span :class="['permission-indicator', { ok: permissions.accessibility }]">
                <UiIcon :name="permissions.accessibility ? 'check' : 'keyboard'" :size="17" />
              </span>
              <div>
                <strong>辅助功能</strong>
                <span>读取当前选中文本；失败时通过复制降级。</span>
              </div>
            </div>
            <button
              :class="permissions.accessibility ? 'secondary-button' : 'primary-button'"
              @click="openPermission('accessibility')"
            >
              {{ permissions.accessibility ? "已授权" : "去授权" }}
            </button>
          </article>
          <article>
            <div class="permission-copy">
              <span :class="['permission-indicator', { ok: permissions.screenRecording }]">
                <UiIcon :name="permissions.screenRecording ? 'check' : 'screenshot'" :size="17" />
              </span>
              <div>
                <strong>屏幕录制</strong>
                <span>捕获当前显示器画面，用于区域截图。</span>
              </div>
            </div>
            <button
              :class="permissions.screenRecording ? 'secondary-button' : 'primary-button'"
              @click="openPermission('screenRecording')"
            >
              {{ permissions.screenRecording ? "已授权" : "去授权" }}
            </button>
          </article>
        </div>
        <button v-if="isMac" class="ghost-button refresh-button" @click="refreshPermissions">
          <UiIcon name="update" />
          已授权，重新检测
        </button>
      </div>

      <div v-else class="step-panel">
        <span class="step-icon"><UiIcon name="keyboard" :size="28" /></span>
        <h2>记住两个快捷键</h2>
        <p>可以在设置中随时修改。首次使用前不需要配置翻译服务，进入应用后再按需添加。</p>
        <div class="hotkey-cards">
          <div class="hotkey-card">
            <span>划词翻译</span>
            <HotkeyRecorder
              v-model="settings.hotkeys.translate"
              label="划词翻译快捷键"
              :conflict-value="settings.hotkeys.screenshot"
            />
            <small>{{ isMac ? "选中文本后按此组合键" : "选中文本后按此组合键" }}</small>
          </div>
          <div class="hotkey-card">
            <span>区域截图</span>
            <HotkeyRecorder
              v-model="settings.hotkeys.screenshot"
              label="区域截图快捷键"
              :conflict-value="settings.hotkeys.translate"
            />
            <small>截取当前显示器区域</small>
          </div>
        </div>
        <label class="launch-option">
          <input v-model="settings.launchAtLogin" type="checkbox" />
          <span>
            <strong>登录后自动启动</strong>
            <small>S-Tools 会安静地待在托盘，不主动打开窗口。</small>
          </span>
        </label>
      </div>

      <div v-if="error" class="onboarding-error">
        <UiIcon name="alert" />
        {{ error }}
      </div>

      <footer class="onboarding-actions">
        <button v-if="step > 0" class="ghost-button" @click="step--">上一步</button>
        <span v-else></span>
        <button v-if="step < 2" class="primary-button" @click="step++">
          继续
          <UiIcon name="arrow" />
        </button>
        <button v-else class="primary-button" :disabled="busy" @click="finish">
          <span v-if="busy" class="spinner"></span>
          {{ busy ? "正在进入…" : "开始使用 S-Tools" }}
        </button>
      </footer>
    </div>
  </main>
</template>

<style scoped>
.onboarding {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  padding: 32px;
  background:
    radial-gradient(circle at 15% 0%, rgba(247, 147, 30, 0.12), transparent 27%),
    radial-gradient(circle at 100% 100%, rgba(92, 145, 255, 0.08), transparent 32%),
    var(--bg);
}

.onboarding-shell {
  width: min(760px, 100%);
  min-height: 620px;
  padding: 30px 34px 26px;
  border: 1px solid var(--line-strong);
  border-radius: 28px;
  background: color-mix(in srgb, var(--bg-elevated) 96%, transparent);
  box-shadow: 0 36px 110px rgba(0, 0, 0, 0.38);
}

.onboarding-head,
.onboarding-actions,
.brand-lockup,
.permission-copy,
.launch-option {
  display: flex;
  align-items: center;
}

.onboarding-head {
  justify-content: space-between;
}

.brand-lockup {
  gap: 14px;
}

.brand-lockup h1 {
  margin: 3px 0 0;
  font-size: 22px;
  letter-spacing: -0.04em;
}

.step-label {
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 650;
  letter-spacing: 0.08em;
}

.step-panel {
  min-height: 450px;
  padding: 54px 12px 36px;
  animation: fade-up 250ms ease both;
}

.step-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  border-radius: 17px;
  background: var(--accent-soft);
  color: var(--accent-strong);
}

.step-panel h2 {
  margin: 20px 0 10px;
  font-size: 32px;
  letter-spacing: -0.05em;
}

.step-panel > p {
  max-width: 580px;
  margin: 0;
  color: var(--text-soft);
  font-size: 13px;
  line-height: 1.8;
}

.feature-pair {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 38px;
}

.feature-pair > div {
  display: grid;
  gap: 10px;
  padding: 22px;
  border: 1px solid var(--line);
  border-radius: 17px;
  background: var(--bg);
  color: var(--accent-strong);
}

.feature-pair strong {
  color: var(--text);
  font-size: 14px;
}

.feature-pair span {
  color: var(--text-muted);
  font-size: 11px;
  line-height: 1.6;
}

.permission-list {
  display: grid;
  gap: 10px;
  margin-top: 32px;
}

.permission-list article {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  padding: 17px;
  border: 1px solid var(--line);
  border-radius: 15px;
  background: var(--bg);
}

.permission-copy {
  gap: 12px;
}

.permission-indicator {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: 11px;
  background: var(--bg-soft);
  color: var(--text-muted);
}

.permission-indicator.ok {
  background: rgba(82, 211, 154, 0.1);
  color: var(--success);
}

.permission-copy strong,
.permission-copy span {
  display: block;
}

.permission-copy strong {
  font-size: 12px;
}

.permission-copy span {
  margin-top: 4px;
  color: var(--text-muted);
  font-size: 10px;
}

.permission-list .primary-button,
.permission-list .secondary-button {
  flex: 0 0 auto;
}

.refresh-button {
  margin-top: 12px;
  padding-left: 4px;
}

.hotkey-cards {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
  margin-top: 36px;
}

.hotkey-card {
  display: grid;
  gap: 12px;
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--bg);
}

.hotkey-card > span {
  color: var(--text-soft);
  font-size: 11px;
  font-weight: 650;
}

.hotkey-card small {
  color: var(--text-muted);
  font-size: 10px;
}

.launch-option {
  gap: 12px;
  margin-top: 20px;
  padding: 16px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--bg);
}

.launch-option input {
  width: 17px;
  height: 17px;
  accent-color: var(--accent);
}

.launch-option strong,
.launch-option small {
  display: block;
}

.launch-option strong {
  font-size: 11px;
}

.launch-option small {
  margin-top: 3px;
  color: var(--text-muted);
  font-size: 10px;
}

.onboarding-error {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 12px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 107, 104, 0.1);
  color: var(--danger);
  font-size: 11px;
}

.onboarding-actions {
  justify-content: space-between;
  padding-top: 20px;
  border-top: 1px solid var(--line);
}

@media (max-width: 700px) {
  .onboarding {
    padding: 18px;
  }

  .onboarding-shell {
    min-height: 560px;
    padding: 24px;
  }

  .feature-pair,
  .hotkey-cards {
    grid-template-columns: 1fr;
  }

  .step-panel {
    padding-top: 34px;
  }

  .step-panel h2 {
    font-size: 26px;
  }
}
</style>
