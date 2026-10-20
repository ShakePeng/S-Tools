# S-Tools

基于 Tauri 2、Vue 3 和 TypeScript 的跨平台桌面工具箱，首版聚焦：

- 划词翻译：快捷键读取选区，支持百度翻译、Google Cloud Translation、腾讯云机器翻译、DeepSeek 和自定义 OpenAI 兼容服务。
- 多服务对照：可同时启用最多 5 个已配置服务，分别展示结果和错误。
- 区域截图：冻结当前显示器画面，悬停识别窗口，单击选择窗口，或拖动框选并八向调整。
- 快速启动：启动时预热隐藏截图窗口，快捷键触发后复用该窗口并直接显示冻结画面。
- 截图工具栏：跟随选区显示在下方，空间不足时自动移动到上方。
- 标注编辑：矩形、椭圆、直线、箭头、画笔、文字和像素/模糊马赛克，支持选择、移动、缩放、撤销和重做。
- 截图输出：复制到剪贴板或原生另存为。
- 本地历史：翻译与截图记录保存在本机 SQLite，可删除或关闭。
- 自动更新：从 GitHub Release 的 `latest-v2.json` 检查更新，校验 SHA256 后安装。

划词翻译和截图默认关闭。进入工作台后按需打开功能开关，只有已开启的功能才会显示在侧边栏导航中；历史记录固定显示在功能导航上方。

自定义 OpenAI 兼容服务支持 HTTP 与 HTTPS Base URL；HTTP 会在配置界面提示 API Key 明文传输风险。

## 开发

环境要求：

- Node.js 22（CI 使用）；本地兼容 Node.js 16 + Vite 4。
- Yarn 1.22。
- Rust stable。
- macOS 需要 Xcode Command Line Tools；Windows 需要 MSVC 与 WebView2。

```bash
yarn install
yarn tauri:dev
```

项目通过根目录 `.yarnrc` 固定使用淘宝 npm 镜像 `https://registry.npmmirror.com`。

常用验证：

```bash
yarn build
yarn test
yarn verify
```

## 默认快捷键

- 划词翻译：macOS `⌥ + D`，Windows `Alt + D`
- 区域截图：`F1`

翻译快捷键与目标语言在「翻译」Tab 中修改；截图快捷键在「截图」Tab 中修改。通用设置只保留应用级选项。

快捷键输入采用录制方式：点击输入框后直接按下新的组合键，应用会自动识别；功能键（如 F1）可以单独使用，普通字母和数字需要搭配至少一个修饰键。

## 权限

macOS 首次使用需要：

- **辅助功能**：读取当前选中文本；失败时使用复制降级。
- **屏幕录制**：截图时捕获当前显示器。

Windows 无需额外授权。所有翻译服务默认关闭，需要用户自行填写服务凭据；密钥保存在 macOS Keychain 或 Windows Credential Manager。

## 数据目录

- macOS：`~/Library/Application Support/cn.shakepeng.stools/`
- Windows：`%APPDATA%\cn.shakepeng.stools\`

目录包含 `config.json`、`s-tools.db` 和 `captures/`。截图不会上传；翻译文本只会发送给用户主动选择的服务。

截图交互和 Canvas 标注模型移植自 MIT 许可的 [Kiri](https://github.com/yuxino/Kiri)，固定来源与许可文本见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。S-Tools 保留自己的 Vue 外壳、品牌和业务流程，不包含 Kiri 的录屏、OCR 或素材库模块。

截图时 S-Tools 不会隐藏主窗口或其他应用窗口，当前桌面会原样进入冻结画面。

## 图标

三套纯 S 缎带候选位于 `design/icon/`：

- 深色橙光 `candidate-a-s-core`
- 蓝紫玻璃 `candidate-b-s-glass`
- 极简黑白 `candidate-c-s-mono`

当前开发阶段暂用深色橙光方案。正式发布前由用户选型，然后执行：

```bash
python3 scripts/generate_icons.py
yarn icon
```

## 构建

```bash
yarn tauri:build
```

macOS Apple Silicon：

```bash
yarn tauri:build:macos
```

Windows NSIS：

```bash
yarn tauri:build:windows
```

## 发布

版本号必须同时保持一致：

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `CHANGELOG.md`

初始版本为 `1.0.0`，对应标签 `v1.0.0`。推送 `v*` 标签后，GitHub Actions 会构建：

- macOS ARM64 DMG
- macOS ARM64 `.app.zip`
- Windows x64 NSIS 安装包
- `latest.json`
- `latest-v2.json`

当前 macOS 使用 ad-hoc 签名、Windows 未签名，首次启动可能出现系统安全提示。ad-hoc 版本升级后，macOS 可能要求重新授予辅助功能和屏幕录制权限。

详细设计见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。
