import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

// Geometry comes from the existing MIT-licensed Phosphor dependency, not vendor marks.
const icons = {
  claude: ["PencilLine", "#de895d"],
  codex: ["TerminalWindow", "#8b9ff4"],
  kimi: ["Compass", "#65b8c9"],
  zcode: ["BracketsCurly", "#af8ce9"],
  deepseek: ["Waveform", "#61b5af"],
  opencode: ["Code", "#88bd94"],
  pi: ["Cube", "#d3b76f"],
  minimax: ["Lightning", "#d98fa5"],
  idle: ["PauseCircle", "#a1a7b6"],
  activity: ["Wind", "#d7a57e"],
};
const directory = new URL("../src/assets/neutral-agents/", import.meta.url);
await mkdir(directory, { recursive: true });
for (const [name, [symbol, color]] of Object.entries(icons)) {
  const module = await import(`@phosphor-icons/react/dist/ssr/${symbol}`);
  const svg = renderToStaticMarkup(createElement(module[`${symbol}Icon`], {
    size: 64, color, weight: "duotone", "aria-hidden": true,
  }));
  await writeFile(new URL(`${name}.svg`, directory), `${svg}\n`);
}
console.log(`Generated ${Object.keys(icons).length} neutral icons in ${fileURLToPath(directory)}`);
