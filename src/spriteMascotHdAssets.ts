/// <reference types="vite/client" />
import clawdIdle from "./assets/mascots-hd/claude-programmer/idle.webp";
import clawdFlow from "./assets/mascots-hd/claude-programmer/motion-flow.png";
import clawdVisibility from "./assets/mascots-hd/claude-programmer/motion-visibility.png";
import clawdManifest from "./assets/mascots-hd/claude-programmer/motion-manifest.json";
import zcodeIdle from "./assets/mascots-hd/zcode-arcade/idle.webp";
import zcodeFlow from "./assets/mascots-hd/zcode-arcade/motion-flow.png";
import zcodeVisibility from "./assets/mascots-hd/zcode-arcade/motion-visibility.png";
import zcodeManifest from "./assets/mascots-hd/zcode-arcade/motion-manifest.json";
import whaleIdle from "./assets/mascots-hd/deepseek-whale-girl/idle.webp";
import whaleFlow from "./assets/mascots-hd/deepseek-whale-girl/motion-flow.png";
import whaleVisibility from "./assets/mascots-hd/deepseek-whale-girl/motion-visibility.png";
import whaleManifest from "./assets/mascots-hd/deepseek-whale-girl/motion-manifest.json";

const paths = import.meta.glob("./assets/mascots-hd/*/motion-*.webp", { eager: true, query: "?url", import: "default" }) as Record<string, string>;
const actions = (stem: string) => Array.from({ length: 8 }, (_, row) => paths[`./assets/mascots-hd/${stem}/motion-${row}.webp`]);
export const CLAWD_HD = { atlas: clawdIdle, flow: { image: clawdFlow, visibility: clawdVisibility, manifest: clawdManifest, actions: actions("claude-programmer") } };
export const ZCODE_HD = { atlas: zcodeIdle, flow: { image: zcodeFlow, visibility: zcodeVisibility, manifest: zcodeManifest, actions: actions("zcode-arcade") } };
export const WHALE_HD = { atlas: whaleIdle, flow: { image: whaleFlow, visibility: whaleVisibility, manifest: whaleManifest, actions: actions("deepseek-whale-girl") } };
