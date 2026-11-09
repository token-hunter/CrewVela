// Read src/styles.css and inline its @import graph in import order,
// so string assertions keep working after the domain split.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const projectRoot = fileURLToPath(new URL("../../", import.meta.url));

export function readStyles(entryUrl = new URL("../../src/styles.css", import.meta.url)) {
  const seen = new Set();
  const walk = (url) => {
    const file = fileURLToPath(url);
    if (seen.has(file)) return "";
    seen.add(file);
    const css = readFileSync(file, "utf8");
    return css.replace(/@import\s+"([^"]+)"\s*;/g, (_match, rel) =>
      // Vite 允许 "/src/..." 形式的根绝对导入；按项目根解析，其余相对当前文件。
      walk(
        pathToFileURL(
          rel.startsWith("/")
            ? resolve(projectRoot, `.${rel}`)
            : resolve(dirname(file), rel),
        ),
      ),
    );
  };
  return walk(entryUrl);
}
