import type { SpriteMascotFrameLayout } from "./spriteMascotTransitions";

/** At most two decoded/uploaded HD action atlases per visible mascot. */
export class SpriteMascotActionTextures {
  private textures = new Map<number, { texture: WebGLTexture; width: number; height: number }>();
  private pending = new Map<number, HTMLImageElement>();
  private requests = new Map<number, AbortController>();
  private failures = new Map<number, Error>();
  private disposed = false;
  constructor(private gl: WebGLRenderingContext, private urls: readonly string[], private layouts?: readonly SpriteMascotFrameLayout[]) {
    if (urls.length !== 8 || urls.some(url => !url)) throw new Error("Eight HD mascot actions are required");
  }
  prepare(rows: readonly number[]) {
    if (this.disposed) return;
    const wanted = new Set(rows.slice(0, 2));
    for (const [row, image] of this.pending) if (!wanted.has(row)) {
      this.requests.get(row)?.abort(); this.requests.delete(row);
      image.onload = image.onerror = null; image.src = ""; this.pending.delete(row);
    }
    for (const [row, entry] of this.textures) if (!wanted.has(row)) {
      this.gl.deleteTexture(entry.texture); this.textures.delete(row);
    }
    for (const row of wanted) {
      if (this.textures.has(row) || this.pending.has(row) || this.failures.has(row)) continue;
      const image = new Image();
      this.pending.set(row, image);
      const fail = (error: Error) => {
        if (this.disposed || this.pending.get(row) !== image) return;
        this.requests.delete(row);
        this.pending.delete(row); this.failures.set(row, error);
        image.onload = image.onerror = null; image.src = "";
        console.warn("HD mascot action could not be loaded.", error);
      };
      image.onerror = () => fail(new Error(`HD mascot action failed to load: ${this.urls[row]}`));
      const request = typeof createImageBitmap === "function" ? new AbortController() : undefined;
      if (request) this.requests.set(row, request);
      const upload = async () => {
        if (this.disposed || this.pending.get(row) !== image) return;
        const gl = this.gl;
        let texture: WebGLTexture | null = null;
        let bitmap: ImageBitmap | undefined;
        try {
          let source: HTMLImageElement | ImageBitmap = image;
          if (request) {
            const response = await fetch(this.urls[row], { signal: request.signal });
            if (!response.ok) throw new Error(`HD mascot action failed to load: ${this.urls[row]} (${response.status})`);
            const blob = await response.blob();
            if (this.disposed || this.pending.get(row) !== image) return;
            // Decode the encoded bytes off-thread. Creating a bitmap from an
            // HTMLImageElement can still synchronously copy a large raster.
            bitmap = await createImageBitmap(blob, { premultiplyAlpha: "premultiply", colorSpaceConversion: "none" });
            source = bitmap;
          } else if (typeof image.decode === "function") await image.decode();
          if (this.disposed || this.pending.get(row) !== image) return;
          const layout = this.layouts?.[row];
          if (layout ? layout.framePixels < 768 || layout.count < 2 || layout.count > layout.columns * layout.rows || source.width !== layout.columns * layout.framePixels || source.height !== layout.rows * layout.framePixels
            : source.width !== source.height * 2 || source.height < 768) throw new Error("Invalid native HD mascot action dimensions");
          const limit = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
          if (source.width > limit || source.height > limit) throw new Error(`HD mascot action exceeds GPU texture limit ${limit}`);
          texture = gl.createTexture();
          if (!texture) throw new Error("Cannot allocate HD mascot action texture");
          gl.activeTexture(gl.TEXTURE3); gl.bindTexture(gl.TEXTURE_2D, texture);
          gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true); gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
          if (gl.getError() !== gl.NO_ERROR) throw new Error("HD mascot action upload failed");
          this.textures.set(row, { texture, width: source.width, height: source.height });
          this.requests.delete(row);
          this.pending.delete(row); image.onload = image.onerror = null; image.src = "";
        } catch (error) { if (texture) gl.deleteTexture(texture); fail(error instanceof Error ? error : new Error(String(error))); }
        finally { bitmap?.close(); }
      };
      if (request) void upload();
      else { image.onload = upload; image.src = this.urls[row]; }
    }
  }
  get(row: number) {
    if (this.failures.has(row)) throw this.failures.get(row)!;
    return this.textures.get(row);
  }
  get stats() {
    return { resident: this.textures.size, pending: this.pending.size,
      bytes: [...this.textures.values()].reduce((total, entry) => total + entry.width * entry.height * 4, 0) };
  }
  dispose() {
    this.disposed = true;
    for (const request of this.requests.values()) request.abort();
    this.requests.clear();
    for (const image of this.pending.values()) { image.onload = image.onerror = null; image.src = ""; }
    this.pending.clear();
    for (const entry of this.textures.values()) this.gl.deleteTexture(entry.texture);
    this.textures.clear(); this.failures.clear();
  }
}
