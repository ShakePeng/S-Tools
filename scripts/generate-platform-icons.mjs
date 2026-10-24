import { mkdirSync, readFileSync, readdirSync, renameSync, rmSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = resolve(root, "src-tauri", "icons", "app-source.png");
const iconsDirectory = resolve(root, "src-tauri", "icons");
const tauri = resolve(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const platforms = [
  ["windows", 42],
  ["macos", 100],
];

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    shell: false,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status != 0) {
    process.exit(result.status ?? 1);
  }
}

function insetIcon(sourcePath, outputPath, inset) {
  const script = `
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Image]::FromFile('${sourcePath.replaceAll("'", "''")}')
$canvas = [System.Drawing.Bitmap]::new(1024, 1024, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($canvas)
$graphics.Clear([System.Drawing.Color]::Transparent)
$graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
$graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$graphics.DrawImage($source, ${inset}, ${inset}, ${1024 - inset * 2}, ${1024 - inset * 2})
$canvas.Save('${outputPath.replaceAll("'", "''")}', [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$canvas.Dispose()
$source.Dispose()
`;
  run("powershell", ["-NoProfile", "-Command", `& {${script}}`]);
}

function outlineWindowsIcon(sourcePath, outputPath) {
  const script = `
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Bitmap]::new('${sourcePath.replaceAll("'", "''")}')
$outline = [System.Drawing.Bitmap]::new(1024, 1024, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($outline)
$graphics.Clear([System.Drawing.Color]::Transparent)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$path = [System.Drawing.Drawing2D.GraphicsPath]::new()
$left = 8.0
$top = 8.0
$right = 1015.0
$bottom = 1015.0
$radius = 209.0
$diameter = $radius * 2
$path.AddArc($left, $top, $diameter, $diameter, 180, 90)
$path.AddArc($right - $diameter, $top, $diameter, $diameter, 270, 90)
$path.AddArc($right - $diameter, $bottom - $diameter, $diameter, $diameter, 0, 90)
$path.AddArc($left, $bottom - $diameter, $diameter, $diameter, 90, 90)
$path.CloseFigure()
$pen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(210, 255, 255, 255), 10)
$graphics.DrawPath($pen, $path)
$graphics.DrawImage($source, 0, 0, 1024, 1024)
$path.Dispose()
$graphics.Dispose()
$outline.Save('${outputPath.replaceAll("'", "''")}', [System.Drawing.Imaging.ImageFormat]::Png)
$pen.Dispose()
$outline.Dispose()
$source.Dispose()
`;
  run("powershell", ["-NoProfile", "-Command", `& {${script}}`]);
}

function assertWindowsIconFrames(iconPath) {
  const bytes = readFileSync(iconPath);
  const count = bytes.readUInt16LE(4);
  const sizes = new Set();
  for (let index = 0; index < count; index++) {
    const offset = 6 + index * 16;
    sizes.add(bytes[offset] || 256);
  }
  for (const size of [16, 32, 48, 64, 256]) {
    if (!sizes.has(size)) {
      throw new Error(`Windows icon is missing ${size}px frame`);
    }
  }
}

for (const [platform, inset] of platforms) {
  const output = resolve(iconsDirectory, platform);
  const scaled = resolve(output, "app-source.png");
  rmSync(output, { recursive: true, force: true });
  mkdirSync(output, { recursive: true });
  insetIcon(source, scaled, inset);
  if (platform == "windows") {
    const decorated = resolve(output, "app-source-outline.png");
    outlineWindowsIcon(scaled, decorated);
    rmSync(scaled, { force: true });
    renameSync(decorated, scaled);
  }
  run(process.execPath, [tauri, "icon", scaled, "--output", output]);
  if (platform == "windows") {
    assertWindowsIconFrames(resolve(output, "icon.ico"));
  }

  const keep = new Set(["app-source.png", "32x32.png", "128x128.png", "128x128@2x.png"]);
  for (const entry of readdirSync(output)) {
    const path = resolve(output, entry);
    if (
      (entry.endsWith(".png") && !keep.has(entry)) ||
      (entry === "icon.ico" && platform == "macos") ||
      (entry === "icon.icns" && platform == "windows") ||
      entry === "android" ||
      entry === "ios"
    ) {
      rmSync(path, { force: true, recursive: true });
    }
  }
}

console.log(
  `Generated platform icons: ${platforms
    .map(([platform]) => relative(root, resolve(iconsDirectory, platform)))
    .join(", ")}`,
);
