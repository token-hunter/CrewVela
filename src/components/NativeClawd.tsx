import atlas from "../assets/claude-programmer-atlas.png";
import { CLAWD_MOTIONS } from "../clawdMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-clawd.css";

export function NativeClawd({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} atlas={atlas} motions={CLAWD_MOTIONS} className="native-clawd" label="Clawd 程序员" />;
}
