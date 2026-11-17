import type { AgentRuntimeKind } from "../types";
import "../styles/cli-session-pet.css";

export type CliPetRuntimeKind = Extract<
  AgentRuntimeKind,
  "codex" | "kimi_code" | "zcode_cli" | "opencode" | "pi"
>;

interface Props {
  runtimeKind: CliPetRuntimeKind;
  working: boolean;
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

function CodexPet() {
  return (
    <svg viewBox="0 0 56 32" aria-hidden="true" focusable="false">
      <path d="M14 25h28v3H14z" fill="#aeb8ca" />
      <path d="M18 22h6v4h-6zm15 0h6v4h-6z" fill="#6c84d5" />
      <path d="M13 3h25v2h3v15h-3v2H13v-2h-3V5h3z" fill="#f2efff" />
      <rect x="13" y="6" width="25" height="13" fill="#6f5be8" />
      <rect x="15" y="8" width="21" height="9" fill="#365be4" />
      <path className="pet-codex-prompt" d="m20 10 3 3-3 3m6 0h5" fill="none" stroke="#fff" strokeWidth="2" />
      <path className="pet-codex-idle-cursor pet-idle-only" d="M21 14h8v2h-8z" fill="#cff8ff" />
      <path className="pet-codex-hand pet-codex-hand-left" d="M10 19h4v4h-4z" fill="#b9a8ff" />
      <path className="pet-codex-hand pet-codex-hand-right" d="M37 19h4v4h-4z" fill="#b9a8ff" />
      <rect x="17" y="25" width="22" height="2" fill="#e4e8f2" />
      <path d="M20 25v2m4-2v2m4-2v2m4-2v2m4-2v2" stroke="#8995aa" strokeWidth="1" />
      <rect className="pet-codex-output pet-codex-output-one" x="29" y="9" width="5" height="2" fill="#76e9ff" />
      <rect className="pet-codex-output pet-codex-output-two" x="29" y="13" width="3" height="2" fill="#b8a7ff" />
      <rect className="pet-codex-output pet-codex-output-three" x="29" y="17" width="6" height="1" fill="#76e9ff" />
    </svg>
  );
}

function KimiPet() {
  return (
    <svg viewBox="0 0 56 32" aria-hidden="true" focusable="false">
      <path
        className="pet-kimi-flight-path"
        d="M38 20C46 14 41 5 32 5"
        fill="none"
        stroke="#ffc735"
        strokeDasharray="2 3"
        strokeWidth="1.5"
      />
      <path d="M10 15h3v-4h3V8h4V5h9v3h3v4h3v8h-3v3h-3v3h-9v-2h-4v-3h-3v-3h-3z" fill="#f0f4ff" />
      <path d="M15 20h4v5h-4zm11 5h4v3h-4z" fill="#80dfff" />
      <path d="M17 7h3V4h4V2h8v3h-3v3h-5v3h-6z" fill="#d8e8ff" />
      <path d="M22 4h4v3h-4zm-2 10h2v3h-2zm7 0h2v3h-2z" fill="#151b2c" />
      <path d="M36 18h3v-3h3v3h4v3h-4v4h-3v-4h-3z" fill="#18d9ef" />
      <path className="pet-kimi-held-star" d="M41 16h2v3h3v2h-3v3h-2v-3h-3v-2h3z" fill="#ffc735" />
      <g className="pet-kimi-flying-star">
        <path d="M41 16h2v3h3v2h-3v3h-2v-3h-3v-2h3z" fill="#ffc735" />
      </g>
      <path className="pet-kimi-catch-arm" d="M35 21h3v3h-3z" fill="#f0f4ff" />
    </svg>
  );
}

function ZCodePet() {
  return (
    <svg viewBox="0 0 56 32" aria-hidden="true" focusable="false">
      <g className="pet-zcode-speed">
        <path d="M3 12h9v2H3zm-2 5h8v2H1zm4 5h8v2H5z" fill="#25d7ff" />
      </g>
      <g className="pet-zcode-brake-pixels" fill="#70eaff">
        <path d="M43 5h3v3h-3zm5 5h3v3h-3zm-1 12h3v3h-3z" />
      </g>
      <g className="pet-zcode-body">
        <path d="M13 3h29v6h-4v4h-5v4h-5v4h14v6H13v-6h4v-4h5v-4h5v-4h5V9H13z" fill="#137cf1" />
        <path d="M15 4h25v2H15v2h20v2h-5v4h-5v4h-5v4H15v-2h5v-4h5v-4h5v-4H15z" fill="#31c8ff" />
        <rect x="20" y="5" width="3" height="3" fill="#091a3a" />
        <rect x="32" y="5" width="3" height="3" fill="#091a3a" />
        <path d="M17 27h7v3h-7zm17 0h7v3h-7z" fill="#1644c8" />
      </g>
    </svg>
  );
}

function OpenCodePet() {
  return (
    <svg viewBox="0 0 56 32" aria-hidden="true" focusable="false">
      <path d="M16 3h23v3h3v17h-3v3H16v-3h-3V6h3z" fill="#f4f0e8" />
      <rect x="17" y="7" width="21" height="13" fill="#181a1e" />
      <path className="pet-opencode-eyes" d="M20 11h2v3h-2zm11 0h2v3h-2z" fill="#f4f0e8" />
      <rect className="pet-opencode-cursor" x="25" y="17" width="5" height="2" fill="#d6d0c5" />
      <path d="M17 24h6v4h-6zm15 0h6v4h-6z" fill="#dcd6ca" />
      <g className="pet-opencode-bracket pet-opencode-bracket-left">
        <path d="m13 12-5 4 5 4m-3-4h6" fill="none" stroke="#f4f0e8" strokeWidth="2" />
      </g>
      <g className="pet-opencode-bracket pet-opencode-bracket-right">
        <path d="m43 12 5 4-5 4m3-4h-6" fill="none" stroke="#f4f0e8" strokeWidth="2" />
      </g>
      <path className="pet-opencode-code-glint" d="M25 8h4v1h-4z" fill="#88e8e0" />
    </svg>
  );
}

function PiPet() {
  return (
    <svg viewBox="0 0 56 32" aria-hidden="true" focusable="false">
      <g className="pet-pi-head">
        <path d="M12 3h22v3h3v10h-3v3H20v8h-8z" fill="#f2eee5" />
        <rect x="18" y="8" width="3" height="3" fill="#17191d" />
        <rect x="26" y="8" width="3" height="3" fill="#17191d" />
      </g>
      <path d="M12 26h7v3h-7zm16 0h7v3h-7z" fill="#d8d1c5" />
      <path d="M31 18h7v4h-7z" fill="#f2eee5" />
      <g className="pet-pi-token">
        <rect x="37" y="15" width="7" height="7" fill="#a6ea37" />
        <path d="M38 16h2v2h-2z" fill="#dcff7f" />
      </g>
      <g className="pet-pi-idle-token pet-idle-only">
        <rect x="39" y="23" width="7" height="7" fill="#a6ea37" />
        <path d="M40 24h2v2h-2z" fill="#dcff7f" />
      </g>
      <path className="pet-pi-catch-spark" d="M46 19h2v2h-2z" fill="#c8ff66" />
    </svg>
  );
}

const PET_DRAWINGS: Record<CliPetRuntimeKind, () => React.JSX.Element> = {
  codex: CodexPet,
  kimi_code: KimiPet,
  zcode_cli: ZCodePet,
  opencode: OpenCodePet,
  pi: PiPet,
};

export function CliSessionPet({ runtimeKind, working, hidden }: Props) {
  const Drawing = PET_DRAWINGS[runtimeKind];
  const description = `${PET_NAMES[runtimeKind]}${working ? "正在执行" : "待命"}`;

  return (
    <span
      className={`cli-session-pet cli-session-pet--${runtimeKind}${working ? " is-running" : " is-idle"}${hidden ? " is-hidden" : ""}`}
      data-runtime={runtimeKind}
      data-status={working ? "running" : "idle"}
      data-hidden={hidden}
      role="img"
      aria-label={description}
      title={description}
    >
      <Drawing />
    </span>
  );
}
