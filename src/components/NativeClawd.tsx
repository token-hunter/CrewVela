import { CLAWD_HD } from "../spriteMascotHdAssets";
import { CLAWD_MOTIONS } from "../clawdMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-clawd.css";

export function NativeClawd({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} {...CLAWD_HD} motions={CLAWD_MOTIONS} className="native-clawd" label="Clawd 程序员" />;
}
