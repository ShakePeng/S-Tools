export interface HotkeySettings {
  translate: string;
  screenshot: string;
}

export interface AppSettings {
  schemaVersion: number;
  hotkeys: HotkeySettings;
  translationEnabled: boolean;
  screenshotEnabled: boolean;
  targetLang: string;
  launchAtLogin: boolean;
  hasCompletedOnboarding: boolean;
}

export interface SettingsInput extends AppSettings {}

export interface PermissionStatus {
  accessibility: boolean;
  screenRecording: boolean;
}

export type ProviderKind =
  | "baidu"
  | "google"
  | "tencent"
  | "deepseek"
  | "openai-compatible";

export interface ProviderView {
  id: string;
  name: string;
  kind: ProviderKind;
  enabled: boolean;
  configured: boolean;
  model: string;
  baseUrl: string;
  description: string;
  credentials: Record<string, string>;
}

export interface ProviderInput {
  id: string;
  name: string;
  kind: ProviderKind;
  enabled: boolean;
  model: string;
  baseUrl: string;
  credentials: Record<string, string>;
}

export interface TranslationRequest {
  requestId: string;
  providerId: string;
  text: string;
  sourceLang: string;
  targetLang: string;
}

export interface ProviderResult {
  requestId: string;
  providerId: string;
  translatedText: string;
  detectedLang: string;
  elapsedMs: number;
}

export interface PendingTranslation {
  text: string;
  source: "accessibility" | "clipboard" | "manual" | "empty";
  notice: string | null;
}

export interface BootstrapData {
  version: string;
  settings: AppSettings;
  providers: ProviderView[];
  permissions: PermissionStatus;
}

export interface CaptureSession {
  sessionId: string;
  monitorId: number;
  displayId: number;
  imagePath: string;
  positionX: number;
  positionY: number;
  width: number;
  height: number;
  scaleFactor: number;
  screenFrame: CaptureRect;
  windowRects: CaptureRect[];
  backingScale: number;
  foregroundAppId: number | null;
}

export interface CaptureRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CaptureAction = "copy" | "save";

export interface CompleteCaptureRequest {
  sessionId: string;
  dataUrl: string;
  action: CaptureAction;
  savePath: string | null;
}

export interface CompleteCaptureResult {
  path: string | null;
  message: string;
}

export interface UpdateInfo {
  version: string;
  notes: string;
  downloadUrl: string;
  fileName: string;
  sha256: string | null;
  selfUpdate: boolean;
  manualDownloadUrl: string | null;
  manualFileName: string | null;
}

export interface UpdateInstallResult {
  installed: boolean;
  manualRequired: boolean;
  savedPath: string | null;
}

export interface UpdateProgressEvent {
  event: "Started" | "Progress" | "Finished";
  contentLength?: number;
  chunkLength?: number;
}
