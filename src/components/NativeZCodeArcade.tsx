import atlas from "../assets/zcode-arcade-atlas.png";
import { ZCODE_ARCADE_MOTIONS } from "../zcodeArcadeMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-zcode-arcade.css";

export function NativeZCodeArcade({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} atlas={atlas} motions={ZCODE_ARCADE_MOTIONS} className="native-zcode-arcade" label="ZCode 电玩小怪" />;
}
