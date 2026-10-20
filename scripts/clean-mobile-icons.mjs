import { rmSync } from "node:fs";
import { resolve } from "node:path";

for (const directory of ["android", "ios"]) {
  rmSync(resolve("src-tauri/icons", directory), { recursive: true, force: true });
}
