"""Reproducible motion data for the existing welcome sprite artwork.

Development-only dependencies: numpy, opencv-python, torch and the official
Practical-RIFE 4.25 model/code pack. Set MASCOT_RIFE_MODEL to its train_log folder.
No Python/OpenCV/model weights are shipped with the application.
The output contains displacement vectors, not enlarged/repainted character art.
Run: python scripts/build-mascot-flow.py
"""
from pathlib import Path
import hashlib
import json
import math
import os
import sys
import types
import importlib.util

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
SIZE = 224
TILE = 120
PAD = 2
COLS = 16
import torch
import torch.nn.functional as F

# The interpolation model predicts backward flow in the *intermediate* image
# domain and an occlusion mask. Inverting ordinary endpoint flow at runtime
# produces duplicate limbs/contours for large movements.
# https://github.com/hzwer/Practical-RIFE (MIT model pack RIFEv4.25_0919.zip)
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
MODEL = None


def warp(image, field):
    height, width = image.shape[-2:]
    y, x = torch.meshgrid(torch.linspace(-1, 1, height, device=image.device),
                          torch.linspace(-1, 1, width, device=image.device), indexing="ij")
    grid = torch.stack((x, y), dim=-1).unsqueeze(0)
    delta = field.permute(0, 2, 3, 1) / torch.tensor([(width-1)/2, (height-1)/2], device=image.device)
    return F.grid_sample(image, grid + delta, mode="bilinear", padding_mode="border", align_corners=True)


def load_model():
    folder = Path(os.environ.get("MASCOT_RIFE_MODEL", ROOT / ".scratch/mascot-smoothness-20261117/rife425/train_log"))
    if not (folder / "IFNet_HDv3.py").is_file():
        raise RuntimeError("Download the official RIFE 4.25 model pack and set MASCOT_RIFE_MODEL to train_log")
    expected = {"flownet.pkl": "6615790efd627772917205db291f51cd392528a157ecbb2ecaeec3bff8eb6de2",
                "IFNet_HDv3.py": "655b4c772b037967b86c2dd31c8fa3b5323b79dd9a0e0088708d89149bbc8a32"}
    for filename, digest in expected.items():
        if hashlib.sha256((folder / filename).read_bytes()).hexdigest() != digest:
            raise RuntimeError(f"Unexpected RIFE 4.25 model/code: {filename}")
    warplayer = types.ModuleType("model.warplayer")
    warplayer.warp = warp
    sys.modules["model"] = types.ModuleType("model")
    sys.modules["model.warplayer"] = warplayer
    spec = importlib.util.spec_from_file_location("mascot_rife", folder / "IFNet_HDv3.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    model = module.IFNet().eval().to(DEVICE)
    params = torch.load(folder / "flownet.pkl", map_location=DEVICE, weights_only=True)
    params = {k.removeprefix("module."): v for k, v in params.items()}
    # The official checkpoint includes training-only teacher/time-estimator
    # heads, explicitly commented out in IFNet's inference implementation.
    model.load_state_dict({k: v for k, v in params.items() if not k.startswith(("teacher.", "caltime."))}, strict=True)
    return model


def build(stem):
    path = ROOT / "src/assets" / f"{stem}-atlas.png"
    atlas = cv2.imread(str(path), cv2.IMREAD_UNCHANGED)
    height, width = atlas.shape[:2]
    # Match the browser's exact fractional cell bounds, rather than rounding
    # each 221.75px cell independently and shifting limbs between frames.
    x, y = np.meshgrid(np.arange(SIZE), np.arange(SIZE))
    frames = []
    for index in range(32):
        cell = cv2.remap(atlas,
            ((index % 4 + (x + .5) / SIZE) * width / 4 - .5).astype(np.float32),
            ((index // 4 + (y + .5) / SIZE) * height / 8 - .5).astype(np.float32),
            cv2.INTER_LINEAR, borderMode=cv2.BORDER_REPLICATE)
        alpha = cell[:, :, 3:4].astype(np.float32) / 255
        rgba = cv2.cvtColor(cell, cv2.COLOR_BGRA2RGBA).astype(np.float32) / 255
        rgba[:, :, :3] *= alpha
        frames.append(cv2.copyMakeBorder(rgba, 16, 16, 16, 16, cv2.BORDER_CONSTANT))
    pairs = set()
    for a in range(32):
        # All authored poses within an action, including reversed/repeated poses.
        for b in range(a // 4 * 4, a // 4 * 4 + 4):
            if a != b:
                pairs.add(tuple(sorted((a, b))))
        # Any interrupted pose can recover into any action's first pose.
        for b in range(0, 32, 4):
            if a != b:
                pairs.add(tuple(sorted((a, b))))
    pairs = sorted(pairs)
    stride = TILE + PAD * 2
    output = np.zeros((math.ceil(len(pairs) / COLS)*stride, COLS*stride, 4), np.uint8)
    masks = np.zeros(output.shape[:2], np.uint8)
    tensor = torch.from_numpy(np.stack(frames)).permute(0, 3, 1, 2).float().to(DEVICE)
    entries = {}
    for i, (a, b) in enumerate(pairs):
        with torch.inference_mode():
            fields, logits, _ = MODEL(torch.cat([tensor[a:a+1], tensor[b:b+1]], dim=1), timestep=.5, scale_list=[16, 8, 4, 2, 1])
            vectors = fields[-1][0, :, 16:-16, 16:-16].permute(1, 2, 0).cpu().numpy()
            mask = logits.sigmoid()[0, 0, 16:-16, 16:-16].cpu().numpy()
        # Adaptive per-pair scale gives subpixel precision, with exactly zero
        # represented by byte 128. Store both directions in one RGBA texture.
        scale = max(1., float(np.max(np.abs(vectors))))
        vectors = cv2.resize(vectors, (TILE, TILE), interpolation=cv2.INTER_AREA)
        encoded = np.clip(np.rint(vectors / scale * 127 + 128), 1, 255).astype(np.uint8)
        encoded = cv2.copyMakeBorder(encoded, PAD, PAD, PAD, PAD, cv2.BORDER_REPLICATE)
        row, col = divmod(i, COLS)
        output[row*stride:(row+1)*stride, col*stride:(col+1)*stride] = encoded
        # Visibility is a soft selection weight, not artwork. Remove model
        # noise and retain 64 levels; the source character colors stay lossless.
        visibility = cv2.GaussianBlur(cv2.resize(mask, (TILE,TILE), interpolation=cv2.INTER_AREA), (3,3), .6)
        visibility = np.rint(np.rint(visibility * 63) / 63 * 255).astype(np.uint8)
        masks[row*stride:(row+1)*stride, col*stride:(col+1)*stride] = cv2.copyMakeBorder(visibility, PAD, PAD, PAD, PAD, cv2.BORDER_REPLICATE)
        entries[f"{a}:{b}"] = [i, round(scale / SIZE / 127, 10)]
        if i % 40 == 0:
            print(f"{stem}: {i+1}/{len(pairs)}", flush=True)
    # OpenCV's writer expects BGRA; vectors are channels, not display colors.
    flow_path = path.with_name(f"{stem}-flow.png")
    cv2.imwrite(str(flow_path), cv2.cvtColor(output, cv2.COLOR_RGBA2BGRA), [cv2.IMWRITE_PNG_COMPRESSION, 9])
    cv2.imwrite(str(path.with_name(f"{stem}-visibility.png")), masks, [cv2.IMWRITE_PNG_COMPRESSION, 9])
    manifest = {"algorithm": "rife4.25-midpoint-backward", "sourceSha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "tile": TILE, "padding": PAD, "columns": COLS,
        "width": output.shape[1], "height": output.shape[0], "pairs": entries}
    flow_path.with_suffix(".json").write_text(json.dumps(manifest, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{stem}: {len(pairs)} intermediate flow pairs, {output.shape[1]}x{output.shape[0]}, {flow_path.stat().st_size:,} bytes")


if __name__ == "__main__":
    cv2.setNumThreads(2)
    torch.set_num_threads(4)
    MODEL = load_model()
    print(f"Computing offline motion data on {DEVICE}", flush=True)
    for name in ("claude-programmer", "zcode-arcade", "deepseek-whale-girl"):
        build(name)
