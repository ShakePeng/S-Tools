import { rmSync } from "node:fs";
import { resolve } from "node:path";

for (const directory of ["android", "ios"]) {
  for (const root of ["src-tauri/icons", "src-tauri/icons/windows", "src-tauri/icons/macos"]) {
    rmSync(resolve(root, directory), { recursive: true, force: true });
  }
}
