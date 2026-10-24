# S-Tools

基于 Tauri 2、Vue 3 和 TypeScript 的跨平台桌面工具箱，首版聚焦：

- 划词翻译：快捷键释放后读取选区，在鼠标位置显示紧凑翻译悬浮框，支持百度翻译、Google Cloud Translation 和自定义 OpenAI 服务。
- 多服务对照：可同时启用最多 5 个已配置服务，分别展示结果和错误。
- 区域截图：冻结当前显示器画面，拖动框选并八向调整。
- 快速启动：启动时预热隐藏截图窗口，快捷键触发后复用该窗口并直接显示冻结画面。
- 截图工具栏：跟随选区显示在下方，空间不足时自动移动到上方。
- 标注编辑：矩形、椭圆、直线、箭头、画笔、文字和像素/模糊马赛克，支持选择、移动、缩放、撤销和重做。
- 截图输出：复制到剪贴板或原生另存为。
- 自动更新：从 GitHub Release 的 `latest-v2.json` 检查更新，校验 SHA256 后安装。

划词翻译和截图默认关闭。进入“概览”后按需打开插件开关，只有已开启的插件才会显示在侧边栏导航中。插件设置与所需权限分别位于“翻译”和“截图”Tab，应用设置与软件更新统一位于“设置”Tab。

自定义 OpenAI 服务支持 HTTP 与 HTTPS Base URL；HTTP 会在配置界面提示 API Key 明文传输风险。

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

macOS 首次开发时，先安装一次本地代码签名身份：

```bash
yarn tauri:dev:setup
yarn tauri:dev
```

初始化脚本会创建并信任本地证书 `S-Tools Local Dev`，需要确认一次 macOS 用户认证弹框。之后 `yarn tauri:dev` 会在每次 Rust 编译完成后自动签名再启动，前端热更新和 Rust 文件监听保持不变，不需要为了辅助功能权限反复打包或重新授权。

首次从 ad-hoc 调试版切换到这个签名身份后，需要在“系统设置 → 隐私与安全性 → 辅助功能”中删除旧的 `S-Tools Dev` 记录。macOS 有时不会自动弹出授权提示，此时点击列表左下角 `+`，选择或拖入：

```text
~/Applications/S-Tools Dev.app
```

打开右侧开关即可。该手动添加只需要执行一次；之后 Rust 代码修改、重新编译和重新启动都会复用该权限。

可选命令：

```bash
# 构建可独立启动的调试 App，使用同一签名身份
yarn tauri:dev:app

# 不经过本地签名，仅用于不需要选区读取的界面调试
yarn tauri:dev:raw

# 删除本地开发证书和登录钥匙串中的私钥
yarn tauri:dev:setup:remove
```

如果本机后续安装了 Apple Development 证书，可通过环境变量切换签名身份：

```bash
S_TOOLS_SIGNING_IDENTITY="Apple Development: Your Name (TEAMID)" yarn tauri:dev
```

### macOS 正式版固定签名

正式版使用固定的自签名证书 `S-Tools Release Signing`。固定证书可以让 macOS 在后续版本中继续识别同一个应用，不再因为 ad-hoc CDHash 变化而重复要求辅助功能和屏幕录制权限。

本机首次安装正式版前，信任仓库中的发布公钥证书：

```bash
yarn tauri:release:trust
```

从 ad-hoc 版本切换到 `1.0.6` 时，需要最后重新授权一次：

1. 先在“系统设置 → 隐私与安全性 → 辅助功能”和“屏幕录制”中删除旧的 S-Tools 记录。
2. 安装 `1.0.6`，保持应用安装路径与后续更新一致。
3. 重新授权辅助功能和屏幕录制。

此后自动更新或在同一路径覆盖安装新版本时，会继续使用固定证书，不需要再次授权。删除或替换签名证书、改变 Bundle ID 或更换应用安装路径都会使权限重新失效。

维护者首次创建发布证书：

```bash
yarn tauri:release:cert
```

私钥、P12 和密码只生成在 Git 忽略的 `release/macos-release-signing/`。公钥证书提交到 `scripts/macos-release-signing/`。GitHub Actions 需要配置以下 Secrets：

- `MACOS_CERTIFICATE`：P12 文件的 Base64 内容。
- `MACOS_CERTIFICATE_PASSWORD`：P12 密码。

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

翻译快捷键与目标语言在「翻译」Tab 中修改；截图快捷键在「截图」Tab 中修改。应用级选项与软件更新位于「设置」Tab。

快捷键输入采用录制方式：点击输入框后直接按下新的组合键，应用会自动识别；功能键（如 F1）可以单独使用，普通字母和数字需要搭配至少一个修饰键。

## 权限

macOS 首次使用需要：

- **辅助功能**：读取当前选中文本；失败时使用复制降级。
- **屏幕录制**：截图时捕获当前显示器。

Windows 无需额外授权。所有翻译服务默认关闭，需要用户自行填写服务凭据；密钥保存在应用数据目录的 `config.json` 中，文件权限限制为当前用户可读写。

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

当前正式版本使用近黑圆角方块与纯白 S 方案。Windows 与 macOS 共用同一视觉母版：

- 圆角底座占画布约 89.8%，四角保留透明安全边距。
- 仅保留黑底与白色 S，不使用渐变、外描边或橙色笔画。

两份产物均由 `design/icon/s-tools-mono.svg` 的同一几何轮廓生成。重新生成两套 ICO、ICNS 和 PNG：

```bash
yarn icon
```

生成结果位于 `src-tauri/icons/windows/` 和 `src-tauri/icons/macos/`，由
`tauri.windows.conf.json`、`tauri.macos.conf.json` 分别引用。`scripts/generate_icons.py`
仅用于重新生成候选方案，不参与发布构建。

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

当前发布版本为 `1.0.18`，对应标签 `v1.0.18`。推送 `v*` 标签后，GitHub Actions 会构建：

- macOS ARM64 DMG
- macOS ARM64 `.app.zip`
- Windows x64 NSIS 安装包
- `latest.json`
- `latest-v2.json`

当前 macOS 使用固定自签名证书、Windows 未签名，首次启动可能出现系统安全提示。签发身份和安装路径保持不变时，macOS 后续版本升级会复用已有辅助功能和屏幕录制授权。

详细设计见 [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)。
