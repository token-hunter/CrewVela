import { WHALE_HD } from "../spriteMascotHdAssets";
import { WHALE_GIRL_HD_MOTIONS } from "../whaleGirlMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-whale-girl.css";

export function NativeWhaleGirl({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} {...WHALE_HD} motions={WHALE_GIRL_HD_MOTIONS} className="native-whale-girl" label="鲸鱼娘" />;
}
