import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauri = path.join(
  root,
  "node_modules",
  ".bin",
  process.platform == "win32" ? "tauri.cmd" : "tauri",
);
const identity = process.env.S_TOOLS_SIGNING_IDENTITY || "S-Tools Local Dev";
const signingConfig = JSON.stringify({
  bundle: {
    macOS: {
      signingIdentity: identity,
    },
  },
});
const args = [
  "build",
  "--debug",
  "--bundles",
  "app",
  "--config",
  path.join(root, "src-tauri", "tauri.dev.conf.json"),
  "--config",
  signingConfig,
  ...process.argv.slice(2),
];

const child = spawn(tauri, args, {
  cwd: root,
  stdio: "inherit",
  shell: process.platform == "win32",
});

child.on("error", (error) => {
  console.error(`启动 Tauri 构建失败：${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : code || 0);
});
