import type { AgentRuntimeKind, SessionStatus } from "../types";
import "../styles/cli-session-pet.css";
import { RUNTIME_LOGOS } from "../runtimeLogos";

export type CliPetRuntimeKind = Extract<
  AgentRuntimeKind,
  "codex" | "kimi_code" | "zcode_cli" | "opencode" | "pi"
>;

interface Props {
  runtimeKind: CliPetRuntimeKind;
  status: SessionStatus;
  hidden: boolean;
}

const PET_NAMES: Record<CliPetRuntimeKind, string> = {
  codex: "Codex",
  kimi_code: "Kimi Code",
  zcode_cli: "ZCode CLI",
  opencode: "OpenCode",
  pi: "Pi",
};

const PET_RUNTIME_KINDS = new Set<string>([
  "codex",
  "kimi_code",
  "zcode_cli",
  "opencode",
  "pi",
]);

export function isCliPetRuntimeKind(kind: AgentRuntimeKind): kind is CliPetRuntimeKind {
  return PET_RUNTIME_KINDS.has(kind);
}

export function CliSessionPet({ runtimeKind, status, hidden }: Props) {
  const running = status === "running";
  const description = `${PET_NAMES[runtimeKind]}${running ? "正在执行" : "待命"}`;

  return (
    <span
      className={`cli-session-pet cli-session-pet--${runtimeKind}${running ? " is-running" : " is-idle"}${hidden ? " is-hidden" : ""}`}
      data-runtime={runtimeKind}
      data-status={running ? "running" : "idle"}
      data-hidden={hidden}
      role="img"
      aria-label={description}
      title={description}
    >
      <img className="cli-neutral-activity" src={RUNTIME_LOGOS[runtimeKind]} alt="" draggable={false} />
    </span>
  );
}
