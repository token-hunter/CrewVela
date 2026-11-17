import type { SpriteMascotPose } from "./spriteMascotMotion";
import { SpriteMascotActionTextures } from "./spriteMascotActionTextures";

export interface SpriteFlowManifest {
  algorithm: string;
  tile: number; padding: number; columns: number; width: number; height: number;
  pairs: Record<string, number[]>;
}
export interface SpriteFlow { image: string; visibility: string; manifest: SpriteFlowManifest; actions?: readonly string[] }

const idlePose = (index: number) => index % 4 === 0 || index % 4 === 3;
/** Every HD action shares exactly the same rest image. Cross-action recovery
 * therefore uses the old action's rest pose without loading a third atlas. */
export function hdSpritePose(pose: SpriteMascotPose) {
  let { from, to } = pose;
  if (Math.floor(from / 4) !== Math.floor(to / 4)) {
    if (idlePose(to)) to = Math.floor(from / 4) * 4;
    else if (idlePose(from)) from = Math.floor(to / 4) * 4;
    else throw new Error("Unsupported HD mascot cross-action pose");
  }
  return { ...pose, from, to };
}

const images = new Map<string, Promise<HTMLImageElement>>();
function loadImage(url: string) {
  let pending = images.get(url);
  if (!pending) {
    pending = new Promise<HTMLImageElement>((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () => { images.delete(url); reject(new Error(`Mascot texture failed to load: ${url}`)); };
      image.src = url;
    });
    images.set(url, pending);
  }
  return pending;
}

const vertex = `attribute vec2 position;
varying vec2 uv;
void main() { uv = vec2((position.x + 1.) * .5, (1. - position.y) * .5); gl_Position = vec4(position, 0., 1.); }`;
const fragment = `precision highp float;
varying vec2 uv;
uniform sampler2D artwork;
uniform sampler2D restArtwork;
uniform sampler2D displacement;
uniform sampler2D visibility;
uniform vec2 atlasSize;
uniform vec2 restSize;
uniform float hd;
uniform vec2 mapSize;
uniform vec2 mapOrigin;
uniform float tile;
uniform float flowScale;
uniform float reverseFlow;
uniform float fromPose;
uniform float toPose;
uniform float progress;
vec4 flow(vec2 p) {
  return (texture2D(displacement, (mapOrigin + clamp(p, 0., 1.) * (tile - 1.) + .5) / mapSize) * 255. - 128.) * flowScale;
}
vec4 sprite(vec2 p, float pose) {
  if (min(p.x,p.y) < 0. || max(p.x,p.y) > 1.) return vec4(0.);
  if (hd > .5 && (pose == 0. || pose == 3.)) {
    return texture2D(restArtwork, clamp(p,.5/restSize,1.-.5/restSize));
  }
  vec2 cell = vec2(mod(pose, 4.), floor(pose / 4.));
  vec2 bounds = vec2(4.,8.);
  if (hd > .5) { cell = vec2(pose - 1.,0.); bounds = vec2(2.,1.); }
  vec2 halfPixel = .5 * bounds / atlasSize;
  return texture2D(artwork, (cell + clamp(p, halfPixel, 1. - halfPixel)) / bounds);
}
void main() {
  if (progress <= 0. || fromPose == toPose) { gl_FragColor = sprite(uv,fromPose); return; }
  if (progress >= 1.) { gl_FragColor = sprite(uv,toPose); return; }
  // Offline intermediate-domain backward flow avoids unstable inverse solves
  // around silhouettes. The visibility mask resolves occluded hands/props.
  vec4 field = flow(uv);
  vec2 a = uv + mix(field.xy,field.zw,reverseFlow) * (2. * progress);
  vec2 b = uv + mix(field.zw,field.xy,reverseFlow) * (2. * (1. - progress));
  float visible = texture2D(visibility, (mapOrigin + clamp(uv,0.,1.) * (tile - 1.) + .5) / mapSize).r;
  visible = mix(visible,1.-visible,reverseFlow);
  float wa = (1. - progress) * clamp(visible,.001,.999);
  float wb = progress * clamp(1. - visible,.001,.999);
  // Uploaded artwork and output are premultiplied, preserving transparent hair
  // edges instead of dark/red fringes while filtering and interpolating.
  gl_FragColor = (sprite(a,fromPose) * wa + sprite(b,toPose) * wb) / (wa + wb);
}`;

export async function createSpriteMascotRenderer(canvas: HTMLCanvasElement, atlas: string, flow: SpriteFlow) {
  const [art, vectors, visibility] = await Promise.all([loadImage(atlas), loadImage(flow.image), loadImage(flow.visibility)]);
  const gl = canvas.getContext("webgl", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false, preserveDrawingBuffer: true });
  if (!gl) throw new Error("WebGL unavailable for continuous mascot animation");
  const resources: { shaders: WebGLShader[]; textures: WebGLTexture[]; buffers: WebGLBuffer[]; programs: WebGLProgram[] } = { shaders: [], textures: [], buffers: [], programs: [] };
  let actions: SpriteMascotActionTextures | undefined;
  const dispose = () => {
    actions?.dispose();
    resources.textures.forEach(t => gl.deleteTexture(t)); resources.buffers.forEach(b => gl.deleteBuffer(b));
    resources.programs.forEach(p => gl.deleteProgram(p)); resources.shaders.forEach(s => gl.deleteShader(s));
    resources.textures.length = resources.buffers.length = resources.programs.length = resources.shaders.length = 0;
  };
  try {
    const compile = (kind: number, code: string) => {
      const shader = gl.createShader(kind);
      if (!shader) throw new Error("Cannot allocate mascot shader");
      resources.shaders.push(shader); gl.shaderSource(shader, code); gl.compileShader(shader);
      if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) || "Mascot shader compilation failed");
      return shader;
    };
    const program = gl.createProgram();
    if (!program) throw new Error("Cannot allocate mascot program");
    resources.programs.push(program);
    gl.attachShader(program, compile(gl.VERTEX_SHADER, vertex)); gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragment)); gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program) || "Mascot shader link failed");
    gl.useProgram(program);
    const buffer = gl.createBuffer();
    if (!buffer) throw new Error("Cannot allocate mascot vertices");
    resources.buffers.push(buffer); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,-1,1,1,-1,1,1]), gl.STATIC_DRAW);
    const position = gl.getAttribLocation(program, "position"); gl.enableVertexAttribArray(position); gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
    const uniform = (name: string) => gl.getUniformLocation(program, name);
    [art, vectors, visibility].forEach((image, index) => {
      const texture = gl.createTexture();
      if (!texture) throw new Error("Cannot allocate mascot texture");
      resources.textures.push(texture); gl.activeTexture(gl.TEXTURE0 + index); gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, index === 0);
      gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      const format = index === 2 ? gl.LUMINANCE : gl.RGBA;
      gl.texImage2D(gl.TEXTURE_2D, 0, format, format, gl.UNSIGNED_BYTE, image);
      gl.uniform1i(uniform(index === 0 ? "artwork" : index === 1 ? "displacement" : "visibility"), index);
    });
    const manifest = flow.manifest;
    if (manifest.algorithm !== "rife4.25-midpoint-backward") throw new Error("Unsupported mascot displacement data");
    const hd = !!flow.actions;
    if ((hd ? art.width !== art.height || art.width < 768 : art.width / 4 !== art.height / 8) || vectors.width !== manifest.width || vectors.height !== manifest.height || visibility.width !== manifest.width || visibility.height !== manifest.height) throw new Error("Invalid mascot texture dimensions");
    if (gl.getError() !== gl.NO_ERROR) throw new Error("Mascot texture upload failed");
    gl.uniform2f(uniform("atlasSize"), art.width, art.height); gl.uniform2f(uniform("mapSize"), vectors.width, vectors.height);
    gl.uniform1f(uniform("tile"), manifest.tile);
    gl.uniform1f(uniform("hd"), hd ? 1 : 0); gl.uniform2f(uniform("restSize"), art.width, art.height); gl.uniform1i(uniform("restArtwork"), 0);
    if (flow.actions) actions = new SpriteMascotActionTextures(gl, flow.actions);
    const locations = ["fromPose", "toPose", "progress", "flowScale", "reverseFlow", "mapOrigin"].map(uniform);
    const artLocation = uniform("artwork"), sizeLocation = uniform("atlasSize");
    const requiresAction = (pose: SpriteMascotPose) => !((idlePose(pose.from) && (pose.mix <= 0 || idlePose(pose.to))) || (pose.mix >= 1 && idlePose(pose.to)));
    return {
      prepare(pose: SpriteMascotPose, motion: number) {
        if (!actions) return;
        const row = Math.floor(hdSpritePose(pose).from / 4);
        actions.prepare([row, row === motion ? (motion + 1) % 8 : motion]);
      },
      readyFor(pose: SpriteMascotPose) {
        if (!actions || !requiresAction(pose)) return true;
        return !!actions.get(Math.floor(hdSpritePose(pose).from / 4));
      },
      get stats() { return actions?.stats; },
      draw(pose: SpriteMascotPose, width: number, height: number) {
        if (gl.isContextLost()) return false;
        if (actions) {
          pose = hdSpritePose(pose);
          const texture = requiresAction(pose) ? actions.get(Math.floor(pose.from / 4)) : undefined;
          if (requiresAction(pose) && !texture) return false;
          if (texture) {
            gl.activeTexture(gl.TEXTURE3); gl.bindTexture(gl.TEXTURE_2D, texture.texture); gl.uniform1i(artLocation, 3);
            gl.uniform2f(sizeLocation, texture.width, texture.height);
          } else { gl.uniform1i(artLocation, 0); gl.uniform2f(sizeLocation, art.width, art.height); }
        }
        const ratio = Math.min(window.devicePixelRatio || 1, 2.5);
        const w = Math.max(1, Math.min(900, Math.round(width * ratio))), h = Math.max(1, Math.min(900, Math.round(height * ratio)));
        if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; gl.viewport(0, 0, w, h); }
        const entry = manifest.pairs[`${Math.min(pose.from, pose.to)}:${Math.max(pose.from, pose.to)}`];
        if (pose.from !== pose.to && !entry) throw new Error(`Missing mascot displacement ${pose.from}:${pose.to}`);
        const [index, scale] = entry ?? [0, 0], stride = manifest.tile + manifest.padding * 2;
        gl.uniform1f(locations[0], hd ? pose.from % 4 : pose.from); gl.uniform1f(locations[1], hd ? pose.to % 4 : pose.to); gl.uniform1f(locations[2], pose.mix);
        gl.uniform1f(locations[3], scale); gl.uniform1f(locations[4], pose.from > pose.to ? 1 : 0);
        gl.uniform2f(locations[5], (index % manifest.columns) * stride + manifest.padding, Math.floor(index / manifest.columns) * stride + manifest.padding);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        return true;
      },
      dispose,
    };
  } catch (error) { dispose(); throw error; }
}
