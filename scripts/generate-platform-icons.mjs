import { mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = resolve(root, "src-tauri", "icons", "app-source.png");
const iconsDirectory = resolve(root, "src-tauri", "icons");
const tauri = resolve(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const platforms = [
  ["windows", 64],
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

for (const [platform, inset] of platforms) {
  const output = resolve(iconsDirectory, platform);
  const scaled = resolve(output, "app-source.png");
  rmSync(output, { recursive: true, force: true });
  mkdirSync(output, { recursive: true });
  insetIcon(source, scaled, inset);
  run(process.execPath, [tauri, "icon", scaled, "--output", output]);

  const keep = new Set(["app-source.png", "32x32.png", "128x128.png", "128x128@2x.png"]);
  for (const entry of readdirSync(output)) {
    const path = resolve(output, entry);
    if (
      (entry.endsWith(".png") && !keep.has(entry)) ||
      (entry === "icon.ico" && platform == "macos") ||
      (entry === "icon.icns" && platform == "windows")
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
