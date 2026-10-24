# S-Tools 图标候选

三套候选使用同一个几何 S 骨架，只改变底色、材质和光影。

- `candidate-a-s-core.svg`：深炭黑 + 白色 S + 橙色内折高光。开发阶段暂用此方案。
- `candidate-b-s-glass.svg`：蓝紫渐变 + 玻璃质感 S。
- `candidate-c-s-mono.svg`：米白底 + 纯黑几何 S。
- `s-tools-mono.svg`：正式图标，近黑圆角底 + 纯白 S。

正式图标从 `s-tools-mono.svg` 的几何轮廓生成，Windows 与 macOS 共用同一母版。

重新生成预览：

```bash
python3 scripts/generate_icons.py
```

脚本需要 Python 3 与 Pillow。正式版图标以 SVG 为母版，PNG/ICNS/ICO 均由母版派生。
