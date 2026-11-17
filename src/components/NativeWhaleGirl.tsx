import atlas from "../assets/deepseek-whale-girl-atlas.png";
import { WHALE_GIRL_MOTIONS } from "../whaleGirlMotion";
import { NativeSpriteMascot } from "./NativeSpriteMascot";
import "../styles/native-whale-girl.css";

export function NativeWhaleGirl({ active = true }: { active?: boolean }) {
  return <NativeSpriteMascot active={active} atlas={atlas} motions={WHALE_GIRL_MOTIONS} className="native-whale-girl" label="鲸鱼娘" />;
}
