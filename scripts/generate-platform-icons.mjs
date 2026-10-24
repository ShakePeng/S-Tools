import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = resolve(root, "design", "icon", "candidate-a-s-core.svg");
const iconsDirectory = resolve(root, "src-tauri", "icons");
const tauri = resolve(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const platforms = ["windows", "macos"];

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

function writeUnifiedIcon(sourcePath, outputPath) {
  const script = `
Add-Type -AssemblyName System.Drawing
$canvas = [System.Drawing.Bitmap]::new(1024, 1024, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($canvas)
$graphics.Clear([System.Drawing.Color]::Transparent)
$graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias

$path = [System.Drawing.Drawing2D.GraphicsPath]::new()
$left = 52.0
$top = 52.0
$right = 972.0
$bottom = 972.0
$radius = 200.0
$diameter = $radius * 2
$path.AddArc($left, $top, $diameter, $diameter, 180, 90)
$path.AddArc($right - $diameter, $top, $diameter, $diameter, 270, 90)
$path.AddArc($right - $diameter, $bottom - $diameter, $diameter, $diameter, 0, 90)
$path.AddArc($left, $bottom - $diameter, $diameter, $diameter, 90, 90)
$path.CloseFigure()
$background = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#17181B'))
$graphics.FillPath($background, $path)

$white = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#F7F5F1'))
$sPath = [System.Drawing.Drawing2D.GraphicsPath]::new()
$sPath.StartFigure()
$sPath.AddBezier(694, 270, 634, 238, 566, 221, 495, 221)
$sPath.AddBezier(495, 221, 360, 221, 254, 310, 254, 430)
$sPath.AddBezier(254, 430, 254, 554, 361, 596, 481, 627)
$sPath.AddBezier(481, 627, 580, 652, 632, 672, 632, 728)
$sPath.AddBezier(632, 728, 632, 780, 575, 809, 507, 809)
$sPath.AddBezier(507, 809, 424, 809, 350, 781, 294, 740)
$sPath.AddLine(294, 740, 260, 794)
$sPath.AddBezier(260, 794, 323, 842, 410, 868, 507, 868)
$sPath.AddBezier(507, 868, 654, 868, 763, 790, 763, 676)
$sPath.AddBezier(763, 676, 763, 558, 669, 514, 546, 483)
$sPath.AddBezier(546, 483, 442, 457, 382, 435, 382, 385)
$sPath.AddBezier(382, 385, 382, 335, 433, 303, 502, 303)
$sPath.AddBezier(502, 303, 558, 303, 614, 316, 660, 340)
$sPath.AddLine(660, 340, 694, 270)
$sPath.CloseFigure()
$graphics.TranslateTransform(512, 512)
$graphics.ScaleTransform(1.12, 1.12)
$graphics.TranslateTransform(-512, -512)
$graphics.FillPath($white, $sPath)
$graphics.ResetTransform()
$canvas.Save('${outputPath.replaceAll("'", "''")}', [System.Drawing.Imaging.ImageFormat]::Png)
$sPath.Dispose()
$white.Dispose()
$background.Dispose()
$path.Dispose()
$graphics.Dispose()
$canvas.Dispose()
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

function assertUnifiedIcon(sourcePath) {
  const script = `
Add-Type -AssemblyName System.Drawing
$image = [System.Drawing.Bitmap]::new('${sourcePath.replaceAll("'", "''")}')
$corner = $image.GetPixel(0, 0)
if ($corner.A -ne 0) { throw 'icon corner must be transparent' }
$background = $image.GetPixel(512, 80)
if ($background.A -ne 255 -or $background.R -ne 23 -or $background.G -ne 24 -or $background.B -ne 27) {
  throw "unexpected background color $($background.R),$($background.G),$($background.B),$($background.A)"
}
$minX = $image.Width
$minY = $image.Height
$maxX = -1
$maxY = -1
for ($y = 0; $y -lt $image.Height; $y++) {
  for ($x = 0; $x -lt $image.Width; $x++) {
    $pixel = $image.GetPixel($x, $y)
    if ($pixel.A -gt 8) {
      if ($x -lt $minX) { $minX = $x }
      if ($x -gt $maxX) { $maxX = $x }
      if ($y -lt $minY) { $minY = $y }
      if ($y -gt $maxY) { $maxY = $y }
    }
    if ($pixel.A -gt 8 -and $pixel.R -gt 100 -and $pixel.R -gt $pixel.B * 1.5 -and $pixel.G -gt 40) {
      throw "unexpected orange pixel at $x,$y"
    }
  }
}
$width = $maxX - $minX + 1
$height = $maxY - $minY + 1
if ($width -lt 900 -or $width -gt 940 -or $height -lt 900 -or $height -gt 940) {
  throw "unexpected visual bounds $width x $height"
}
$image.Dispose()
`;
  run("powershell", ["-NoProfile", "-Command", `& {${script}}`]);
}

function writeIconPreview(sourcePath, outputPath) {
  const script = `
Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Bitmap]::new('${sourcePath.replaceAll("'", "''")}')
$canvas = [System.Drawing.Bitmap]::new(840, 620, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$graphics = [System.Drawing.Graphics]::FromImage($canvas)
$graphics.Clear([System.Drawing.ColorTranslator]::FromHtml('#232A32'))
$light = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#F4F4F5'))
$graphics.FillRectangle($light, 420, 0, 420, 620)
$graphics.DrawImage($source, 50, 40, 256, 256)
$graphics.DrawImage($source, 470, 40, 256, 256)
$graphics.DrawImage($source, 330, 50, 64, 64)
$graphics.DrawImage($source, 750, 50, 64, 64)
$graphics.DrawImage($source, 330, 140, 32, 32)
$graphics.DrawImage($source, 750, 140, 32, 32)
$graphics.DrawImage($source, 380, 150, 16, 16)
$graphics.DrawImage($source, 800, 150, 16, 16)
$graphics.DrawImage($source, 50, 350, 128, 128)
$graphics.DrawImage($source, 470, 350, 128, 128)
$graphics.DrawImage($source, 220, 380, 48, 48)
$graphics.DrawImage($source, 640, 380, 48, 48)
$canvas.Save('${outputPath.replaceAll("'", "''")}', [System.Drawing.Imaging.ImageFormat]::Png)
$light.Dispose()
$graphics.Dispose()
$canvas.Dispose()
$source.Dispose()
`;
  run("powershell", ["-NoProfile", "-Command", `& {${script}}`]);
}

for (const platform of platforms) {
  const output = resolve(iconsDirectory, platform);
  const scaled = resolve(output, "app-source.png");
  rmSync(output, { recursive: true, force: true });
  mkdirSync(output, { recursive: true });
  writeUnifiedIcon(source, scaled);
  assertUnifiedIcon(scaled);
  copyFileSync(scaled, resolve(iconsDirectory, "app-source.png"));
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

writeIconPreview(
  resolve(iconsDirectory, "windows", "app-source.png"),
  resolve(root, "design", "icon", "s-tools-mono-preview.png"),
);

console.log(
  `Generated platform icons: ${platforms
    .map((platform) => relative(root, resolve(iconsDirectory, platform)))
    .join(", ")}`,
);
