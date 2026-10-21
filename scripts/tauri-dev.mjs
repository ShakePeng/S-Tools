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
const args = ["dev"];
const env = { ...process.env };

if (process.platform == "darwin") {
  const target = {
    arm64: "aarch64-apple-darwin",
    x64: "x86_64-apple-darwin",
  }[process.arch];
  if (!target) {
    console.error(`不支持的 macOS 架构：${process.arch}`);
    process.exit(1);
  }

  const runner = path.join(root, "scripts", "macos-dev-sign-runner.sh");
  const runnerVariable = `CARGO_TARGET_${target.toUpperCase().replaceAll("-", "_")}_RUNNER`;
  env[runnerVariable] = runner;
  args.push("--config", path.join(root, "src-tauri", "tauri.dev.conf.json"));
}

args.push(...process.argv.slice(2));

const child = spawn(tauri, args, {
  cwd: root,
  env,
  stdio: "inherit",
  shell: process.platform == "win32",
});

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => child.kill(signal));
}

child.on("error", (error) => {
  console.error(`启动 Tauri 失败：${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  process.exit(signal ? 1 : code || 0);
});
