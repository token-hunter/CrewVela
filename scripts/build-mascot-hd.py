"""Pack approved native HD imagegen sprites and compute matching motion data.

Inputs: .scratch/mascot-hd-20261117/source-batch-*.json; each generated pair
must contain two square >=768px full-body cells. No enlargement is permitted.
Outputs: lossless WebP action pairs + one shared rest image per character,
and compact motion/visibility maps. The existing RIFE tooling is dev-only.
"""
from pathlib import Path
import importlib.util
import hashlib
import json
import shutil
import sys

import cv2
import numpy as np
from PIL import Image
import torch

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / ".scratch/mascot-hd-20261117"
spec = importlib.util.spec_from_file_location("mascot_flow", ROOT / "scripts/build-mascot-flow.py")
flow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(flow)


def tensor_frame(image):
    rgba = np.array(image.resize((224,224),Image.Resampling.LANCZOS)).astype(np.float32)/255
    rgba[:,:,:3] *= rgba[:,:,3:4]
    return torch.from_numpy(cv2.copyMakeBorder(rgba,16,16,16,16,cv2.BORDER_CONSTANT)).permute(2,0,1).unsqueeze(0).to(flow.DEVICE)


def build(stem, entries, model):
    target = ROOT / "src/assets/mascots-hd" / stem
    target.mkdir(parents=True,exist_ok=True)
    masters = WORK / "masters"
    masters.mkdir(exist_ok=True)
    pairs = []
    audit = []
    for row in range(8):
        entry = entries.get((stem,row))
        if entry is None:
            raise ValueError(f"Missing native HD poses: {stem}/{row}")
        original = Path(entry["source"])
        saved = masters / f"{stem}-{row}.png"
        if not saved.exists() or hashlib.sha256(saved.read_bytes()).digest()!=hashlib.sha256(original.read_bytes()).digest():
            shutil.copy2(original,saved)
        image = Image.open(saved).convert("RGBA")
        w,h=image.size
        if w != 2*h or h < 768:
            raise ValueError(f"Native HD source must be 2:1 with >=768px cells: {saved} {w}x{h}")
        # Keep the native detail and alpha. This is lossless format conversion,
        # not super-resolution, sharpening, or an enlarged old sprite atlas.
        image.save(target / f"action-{row}.webp",lossless=True,exact=True,method=6)
        if not np.array_equal(np.array(image),np.array(Image.open(target/f"action-{row}.webp").convert("RGBA"))):
            raise ValueError(f"HD lossless export changed source pixels: {stem}/{row}")
        pairs.append((image.crop((0,0,h,h)),image.crop((h,0,w,h))))
        audit.append({"row":row,"nativeFramePixels":h,"sourceSha256":hashlib.sha256(saved.read_bytes()).hexdigest(),
                      "assetSha256":hashlib.sha256((target/f"action-{row}.webp").read_bytes()).hexdigest(),"prompt":entry["prompt"]})
    idle=pairs[0][0]
    idle.save(target/"idle.webp",lossless=True,exact=True,method=6)
    frames=[]
    for row,pair in enumerate(pairs):
        frames.extend([idle, idle if row==0 else pair[0], pair[1],idle])
    tiles=120;pad=2;stride=tiles+pad*2;columns=8
    keys=[(row*4+a,row*4+b)for row in range(8)for a in range(4)for b in range(a+1,4)]
    vectors_png=np.zeros((6*stride,columns*stride,4),np.uint8)
    masks_png=np.zeros(vectors_png.shape[:2],np.uint8)
    tensors=[tensor_frame(image) for image in frames]
    metadata={}
    for index,(a,b) in enumerate(keys):
        if np.array_equal(np.array(frames[a]),np.array(frames[b])):
            vectors=np.zeros((224,224,4),np.float32);mask=np.full((224,224),.5,np.float32)
        else:
            with torch.inference_mode():
                fields,logits,_=model(torch.cat([tensors[a],tensors[b]],1),timestep=.5,scale_list=[16,8,4,2,1])
                vectors=fields[-1][0,:,16:-16,16:-16].permute(1,2,0).cpu().numpy()
                mask=logits.sigmoid()[0,0,16:-16,16:-16].cpu().numpy()
        scale=max(1.,float(np.max(np.abs(vectors))))
        encoded=np.clip(np.rint(cv2.resize(vectors,(tiles,tiles),interpolation=cv2.INTER_AREA)/scale*127+128),1,255).astype(np.uint8)
        visible=cv2.GaussianBlur(cv2.resize(mask,(tiles,tiles),interpolation=cv2.INTER_AREA),(3,3),.6)
        visible=np.rint(np.rint(visible*63)/63*255).astype(np.uint8)
        r,c=divmod(index,columns)
        vectors_png[r*stride:(r+1)*stride,c*stride:(c+1)*stride]=cv2.copyMakeBorder(encoded,pad,pad,pad,pad,cv2.BORDER_REPLICATE)
        masks_png[r*stride:(r+1)*stride,c*stride:(c+1)*stride]=cv2.copyMakeBorder(visible,pad,pad,pad,pad,cv2.BORDER_REPLICATE)
        metadata[f"{a}:{b}"]=[index,round(scale/224/127,10)]
    cv2.imwrite(str(target/"flow.png"),cv2.cvtColor(vectors_png,cv2.COLOR_RGBA2BGRA),[cv2.IMWRITE_PNG_COMPRESSION,9])
    cv2.imwrite(str(target/"visibility.png"),masks_png,[cv2.IMWRITE_PNG_COMPRESSION,9])
    manifest={"algorithm":"rife4.25-midpoint-backward","layout":"action-pairs","framePixels":idle.width,
              "tile":tiles,"padding":pad,"columns":columns,"width":vectors_png.shape[1],"height":vectors_png.shape[0],"pairs":metadata,
              "sources":[{k:v for k,v in row.items()if k!='prompt'}for row in audit]}
    (target/"manifest.json").write_text(json.dumps(manifest,separators=(",",":"))+"\n",encoding="utf-8")
    (target/"generation.json").write_text(json.dumps({"mode":"built-in imagegen","nativeFramePixels":idle.width,"actions":audit},ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    (WORK/f"{stem}-asset-audit.json").write_text(json.dumps(audit,ensure_ascii=False,indent=2),encoding="utf-8")
    print(f"{stem}: eight native HD actions ({idle.width}px/frame), shared idle, 48 motion pairs",flush=True)


if __name__=="__main__":
    cv2.setNumThreads(2);torch.set_num_threads(4)
    entries={}
    for file in sorted(WORK.glob("source-batch-*.json")):
        for item in json.loads(file.read_text(encoding="utf-8")):
            entries[(item["stem"],item["row"])]=item
    names=sys.argv[1:] or ["claude-programmer","zcode-arcade","deepseek-whale-girl"]
    # Validate completeness before modifying any production asset.
    for stem in names:
        for row in range(8):
            if (stem,row)not in entries:raise ValueError(f"Missing imagegen asset {stem}/{row}")
    model=flow.load_model()
    for stem in names:build(stem,entries,model)
