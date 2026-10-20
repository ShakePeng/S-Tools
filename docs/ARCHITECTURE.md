# S-Tools 架构说明

## 应用模型

S-Tools 是单进程、多窗口的 Tauri 应用。Rust 侧维护配置、查询、截图、网络和更新状态；Vue 侧只负责界面与交互。

窗口：

- `main`：插件概览、翻译、截图和设置。
- `onboarding`：首次启动权限与快捷键引导。
- `translate`：置顶划词翻译浮窗。
- `capture-overlay`：启动时预热、跨截图会话复用的冻结画面与编辑器窗口。

功能开关：

- `translationEnabled` 与 `screenshotEnabled` 默认均为 `false`。
- 只有开启的功能才会显示在主窗口导航中。
- 关闭功能时同步注销对应全局快捷键，并从导航隐藏插件 Tab。
- 插件开关只位于“概览”；翻译快捷键和目标语言位于翻译 Tab，截图快捷键位于截图 Tab。
- 应用级设置与软件更新合并到“设置”Tab。

## 目录

```text
src/
  shell/                托盘入口、主窗口、首次启动引导
  modules/translate/    划词翻译浮窗
  modules/screenshot/   截图编辑
  components/           通用品牌与图标组件
  lib/                  IPC、格式化与类型工具
src-tauri/src/
  config.rs             JSON 配置与原子写入
  maintenance.rs        旧历史数据清理
  secrets.rs            macOS Keychain / Windows Credential Manager
  selection.rs          Accessibility / UIA / 复制降级
  translate.rs          翻译 provider adapter
  capture.rs            屏幕捕获、导出和剪贴板
  updater.rs            更新清单、SHA256 与平台安装
  platform.rs           窗口枚举、前台应用恢复与跨平台 DPI 适配
  platform/macos.m      macOS Accessibility 与剪贴板桥接
```

## 翻译接口

前端为每个已选 provider 分别调用 `translate`：

```ts
interface TranslationRequest {
  requestId: string;
  providerId: string;
  text: string;
  sourceLang: string;
  targetLang: string;
}
```

每个请求拥有独立取消令牌。provider 完成或失败后分别更新对应结果卡片，不阻塞其他服务。

凭据输入只进入系统凭据库，`ProviderView` 仅返回 `configured` 布尔值。

## 截图接口

`start_capture` 在 Rust 中：

1. 记录前台应用，但保持 S-Tools 和应用窗口原样显示。
2. 获取鼠标所在显示器；macOS 使用系统原生 `screencapture` 捕获，Windows 使用 `xcap`。
3. 枚举显示器内可见窗口，按前后层级裁剪到显示器局部坐标。
4. 保存临时 PNG，并将当前会话写入 Rust 状态。
5. 通知已预热并复用的 `capture-overlay` 窗口。
6. 前端读取原始 PNG 字节并创建 Blob URL，完成解码后发送 ready 信号，Rust 才显示覆盖窗口。
7. 若 3 秒内未就绪，自动隐藏窗口、清理临时文件并发送结束事件。

应用启动时预创建隐藏的 `capture-overlay` WebView，后续截图不再重复初始化窗口。前端优先通过 Tauri asset 协议直接加载临时 PNG，避免大图片 IPC；asset 加载失败时再使用 `get_capture_image_bytes` 原始字节兜底。完成框选与标注后仍导出 PNG Data URL 给 Rust。Rust 负责：

- 解码 PNG。
- 图片写入系统剪贴板。
- 复制到系统图片目录。
- 清理临时截图。

截图坐标按显示器 scale factor 从 CSS 逻辑像素映射到原图像素。macOS 使用显示器点坐标，Windows 将物理窗口矩形统一换算为逻辑点；副屏原点和负坐标保留在 `screenFrame` 中。首版只允许鼠标所在显示器内框选。工具栏以实际 DOM 尺寸跟随选区，优先显示在选区下方，空间不足时回退到上方。

前端以 Kiri 的共享标注模型和 Canvas 渲染管线处理窗口悬停、选区移动/八向缩放、标注撤销/重做、文字排版、马赛克和最终 PNG。截图会话、剪贴板和另存为仍由 S-Tools 管理。

## 配置与隐私

- 非敏感设置：应用数据目录 `config.json`。
- API 凭据：系统凭据库。
- 截图临时文件：应用数据目录 `captures/`。
- 旧版历史数据库和 `captures/history` 会在启动时删除，之后不再记录翻译或截图历史。
- 不采集遥测、不记录翻译正文、不记录截图内容或密钥。

## 更新

更新器读取 GitHub Release 的公开清单，不调用 GitHub API：

- `latest-v2.json`：带 SHA256 的自更新清单。
- `latest.json`：旧格式手动下载回退。

macOS 自更新使用 `.app.zip`，替换失败时回滚旧 `.app`；Windows 使用 NSIS `/S /R`。

## 已知边界

- 首版只构建 macOS ARM64 与 Windows x64。
- macOS ad-hoc 签名升级后可能触发权限重新授权。
- 自定义 OpenAI Base URL 支持 HTTP 与 HTTPS；使用 HTTP 时界面会提示 API Key 将明文传输。
- 跨屏框选、滚动截图、OCR 和取色器不在首版范围。
- Kiri 来源固定在 `52d2d2f8fd613ce3341cbfae371009f06b123903`，不跟随上游自动更新。
