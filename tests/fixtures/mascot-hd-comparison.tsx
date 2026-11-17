import { createRoot } from "react-dom/client";
import { NativeSpriteMascot } from "../../src/components/NativeSpriteMascot";
import { CLAWD_HD, ZCODE_HD, WHALE_HD } from "../../src/spriteMascotHdAssets";
import { CLAWD_MOTIONS } from "../../src/clawdMotion";
import { ZCODE_ARCADE_MOTIONS } from "../../src/zcodeArcadeMotion";
import { WHALE_GIRL_MOTIONS } from "../../src/whaleGirlMotion";
import clawd from "../../src/assets/claude-programmer-atlas.png";
import zcode from "../../src/assets/zcode-arcade-atlas.png";
import whale from "../../src/assets/deepseek-whale-girl-atlas.png";
import "../../src/styles.css";
import "../../src/styles/native-clawd.css";
import "../../src/styles/native-zcode-arcade.css";
import "../../src/styles/native-whale-girl.css";
const rows=[{name:"Clawd",stem:"claude-programmer",className:"native-clawd",old:clawd,hd:CLAWD_HD,motions:CLAWD_MOTIONS},
  {name:"ZCode",stem:"zcode-arcade",className:"native-zcode-arcade",old:zcode,hd:ZCODE_HD,motions:ZCODE_ARCADE_MOTIONS},
  {name:"鲸鱼娘",stem:"deepseek-whale-girl",className:"native-whale-girl",old:whale,hd:WHALE_HD,motions:WHALE_GIRL_MOTIONS}];
createRoot(document.getElementById("root")!).render(<main><h1>相同显示尺寸 · 高清素材对比</h1><p>左：原单帧约 222px；右：新原生单帧 887px。两侧均使用实际欢迎页角色组件，显示区域相同。</p>
  {rows.map(row=><section key={row.stem} data-comparison={row.stem}><h2>{row.name}</h2><div className="pair">{[false,true].map(hd=><article key={String(hd)} className="native-agent-welcome"><h3>{hd?"高清 · 887px":"原素材 · 约 222px"}</h3><div className="native-agent-character">
    <NativeSpriteMascot active={false} atlas={hd?row.hd.atlas:row.old} flow={hd?row.hd.flow:{...row.hd.flow,actions:undefined}} motions={row.motions} className={row.className} label={row.name}/>
  </div></article>)}</div></section>)}</main>);
const style=document.createElement("style");style.textContent="body{margin:0;background:#141413;color:#eee;font:15px system-ui}main{max-width:1000px;margin:auto;padding:24px}h1{font-size:24px;margin:0}p{color:#aaa}h2{font-size:21px;margin:14px 0}h3{font-size:16px;margin:0;color:#ccc}.pair{display:grid;grid-template-columns:1fr 1fr;gap:20px}.pair article{padding:12px;border:1px solid #333;border-radius:14px;gap:12px}.pair .native-agent-character{width:300px!important;height:300px!important;aspect-ratio:1}section{margin:20px 0}";document.head.append(style);
