import { ZCODE_HD } from "../spriteMascotHdAssets";
import { ZCODE_ARCADE_MOTIONS } from "../zcodeArcadeMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-zcode-arcade.css";

export function NativeZCodeArcade({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} {...ZCODE_HD} motions={ZCODE_ARCADE_MOTIONS} className="native-zcode-arcade" label="ZCode 电玩小怪" />;
}
