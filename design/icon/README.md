# S-Tools 图标候选

三套候选使用同一个几何 S 骨架，只改变底色、材质和光影。

- `candidate-a-s-core.svg`：深炭黑 + 白色 S + 橙色内折高光。开发阶段暂用此方案。
- `candidate-b-s-glass.svg`：蓝紫渐变 + 玻璃质感 S。
- `candidate-c-s-mono.svg`：米白底 + 纯黑几何 S。

正式发布前由用户选择一套，替换 `src-tauri/icons/app-source.png` 后重新运行 `yarn icon`。

重新生成预览：

```bash
python3 scripts/generate_icons.py
```

脚本需要 Python 3 与 Pillow。正式版图标以 SVG 为母版，PNG/ICNS/ICO 均由母版派生。
