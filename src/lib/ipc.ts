import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { BootstrapData } from "../types";

export function isTauri(): boolean {
  return Boolean(window.__TAURI_INTERNALS__);
}

export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri()) {
    throw new Error("当前为浏览器预览模式，桌面能力不可用");
  }
  return invoke<T>(command, args);
}

export function assetUrl(path: string): string {
  if (!path || !isTauri()) return "";
  return convertFileSrc(path);
}

export const mockBootstrap: BootstrapData = {
  version: "1.0.0",
  settings: {
    schemaVersion: 1,
    hotkeys: { translate: "Alt+D", screenshot: "F1" },
    translationEnabled: false,
    screenshotEnabled: false,
    targetLang: "auto",
    launchAtLogin: false,
    theme: "dark",
    hasCompletedOnboarding: true,
  },
  providers: [
    {
      id: "baidu",
      name: "百度翻译",
      kind: "baidu",
      enabled: false,
      configured: false,
      model: "",
      baseUrl: "",
      description: "百度翻译开放平台，适合中文语境。",
      credentials: {},
    },
    {
      id: "google",
      name: "Google 翻译",
      kind: "google",
      enabled: false,
      configured: false,
      model: "",
      baseUrl: "",
      description: "Google Cloud Translation，读取系统代理。",
      credentials: {},
    },
    {
      id: "tencent",
      name: "腾讯云翻译",
      kind: "tencent",
      enabled: false,
      configured: false,
      model: "",
      baseUrl: "",
      description: "腾讯云机器翻译，支持 TC3 签名。",
      credentials: {},
    },
    {
      id: "deepseek",
      name: "DeepSeek",
      kind: "deepseek",
      enabled: false,
      configured: false,
      model: "deepseek-chat",
      baseUrl: "https://api.deepseek.com",
      description: "更自然的上下文翻译与语气润色。",
      credentials: {},
    },
    {
      id: "openai",
      name: "OpenAI 兼容",
      kind: "openai-compatible",
      enabled: false,
      configured: false,
      model: "",
      baseUrl: "",
      description: "接入任意兼容 Chat Completions 的服务。",
      credentials: {},
    },
  ],
  permissions: { accessibility: false, screenRecording: false },
};

export async function bootstrap(): Promise<BootstrapData> {
  if (!isTauri()) return mockBootstrap;
  return call<BootstrapData>("bootstrap");
}
