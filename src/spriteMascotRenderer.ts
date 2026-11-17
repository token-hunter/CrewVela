import type { SpriteMascotPose } from "./spriteMascotMotion";
import { SpriteMascotActionTextures } from "./spriteMascotActionTextures";
import { spriteMascotTransition, spriteMascotRigidTransform, type SpriteMascotTransitionData, type SpriteMascotRigidGuide } from "./spriteMascotTransitions";

export interface SpriteFlowManifest {
  algorithm: string;
  tile: number; padding: number; columns: number; width: number; height: number;
  pairs: Record<string, number[]>;
  transitions?: SpriteMascotTransitionData;
  rigidGuides?: Record<string, SpriteMascotRigidGuide>;
  visibilityChannels?: number;
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
uniform vec2 nextMapOrigin;
uniform float fieldMix;
uniform float temporal;
uniform vec2 frameGrid;
uniform float rigidMode;
uniform float rigidCutout;
uniform float rigidOpaque;
uniform float matteGreen;
uniform vec2 rigidMaskFrom;
uniform vec2 rigidMaskTo;
uniform mat3 rigidFrom;
uniform mat3 rigidTo;
uniform mat3 rigidTarget;
uniform vec4 rigidBoundsFrom;
uniform vec4 rigidBoundsTo;
uniform vec4 rigidBoundsTarget;
uniform float tile;
uniform float flowScale;
uniform float reverseFlow;
uniform float fromPose;
uniform float toPose;
uniform float progress;
vec4 flow(vec2 p) {
  vec2 point = clamp(p, 0., 1.) * (tile - 1.) + .5;
  vec4 value = texture2D(displacement, (mapOrigin + point) / mapSize);
  if (temporal > .5) value = mix(value, texture2D(displacement, (nextMapOrigin + point) / mapSize), fieldMix);
  return (value * 255. - 128.) * flowScale;
}
vec4 sprite(vec2 p, float pose) {
  if (min(p.x,p.y) < 0. || max(p.x,p.y) > 1.) return vec4(0.);
  if (hd > .5 && (pose == 0. || (temporal < .5 && pose == 3.))) {
    return texture2D(restArtwork, clamp(p,.5/restSize,1.-.5/restSize));
  }
  vec2 cell = vec2(mod(pose, 4.), floor(pose / 4.));
  vec2 bounds = vec2(4.,8.);
  if (hd > .5) { cell = vec2(mod(pose - 1.,frameGrid.x),floor((pose - 1.)/frameGrid.x)); bounds = frameGrid; }
  vec2 halfPixel = .5 * bounds / atlasSize;
  return texture2D(artwork, (cell + clamp(p, halfPixel, 1. - halfPixel)) / bounds);
}
float card(vec2 p, mat3 transform, vec4 bounds) {
  vec2 q = (transform * vec3(p,1.)).xy;
  q = (q - bounds.xy) / (bounds.zw - bounds.xy);
  if (rigidCutout > .5) return step(0.,q.x)*step(0.,q.y)*step(q.x,1.)*step(q.y,1.);
  vec2 radius = vec2(.045,.10);
  vec2 corner = max(radius - min(q,1. - q),0.) / radius;
  return step(0.,q.x)*step(0.,q.y)*step(q.x,1.)*step(q.y,1.)*(1.-smoothstep(.90,1.02,length(corner)));
}
float part(vec2 p, mat3 transform, vec4 bounds, vec2 origin) {
  vec2 q = (transform * vec3(p,1.)).xy;
  q = (q - bounds.xy)/(bounds.zw-bounds.xy);
  if (min(q.x,q.y)<0. || max(q.x,q.y)>1.) return 0.;
  vec4 value = texture2D(visibility,(origin+q*(tile-1.)+.5)/mapSize);
  return mix(value.r,value.g,matteGreen);
}
void main() {
  float targetCard = rigidMode > .5 ? card(uv,rigidTarget,rigidBoundsTarget) : 0.;
  if (rigidMode > 1.5 && targetCard <= 0.) discard;
  if (progress <= 0. || fromPose == toPose) { gl_FragColor = sprite(uv,fromPose); return; }
  if (progress >= 1.) { gl_FragColor = sprite(uv,toPose); return; }
  // Offline intermediate-domain backward flow avoids unstable inverse solves
  // around silhouettes. The visibility mask resolves occluded hands/props.
  vec4 field = flow(uv);
  vec2 a = uv + mix(field.xy,field.zw,reverseFlow) * mix(2. * progress,1.,temporal);
  vec2 b = uv + mix(field.zw,field.xy,reverseFlow) * mix(2. * (1. - progress),1.,temporal);
  float visible = texture2D(visibility, (mapOrigin + clamp(uv,0.,1.) * (tile - 1.) + .5) / mapSize).r;
  if (temporal > .5) visible = mix(visible, texture2D(visibility, (nextMapOrigin + clamp(uv,0.,1.) * (tile - 1.) + .5) / mapSize).r,fieldMix);
  visible = mix(visible,1.-visible,reverseFlow);
  float wa = mix((1. - progress) * clamp(visible,.001,.999),visible,temporal);
  float wb = mix(progress * clamp(1. - visible,.001,.999),1. - visible,temporal);
  if (rigidMode > .5 && rigidMode < 1.5) {
    float oldA = rigidCutout > .5 ? part(a,rigidFrom,rigidBoundsFrom,rigidMaskFrom) : card(a,rigidFrom,rigidBoundsFrom);
    float oldB = rigidCutout > .5 ? part(b,rigidTo,rigidBoundsTo,rigidMaskTo) : card(b,rigidTo,rigidBoundsTo);
    if (oldA > .5 && oldB < .5) { wa = 0.; wb = 1.; }
    if (oldB > .5 && oldA < .5) { wa = 1.; wb = 0.; }
  }
  // Uploaded artwork and output are premultiplied, preserving transparent hair
  // edges instead of dark/red fringes while filtering and interpolating.
  float alphaA = rigidMode > 1.5 && rigidCutout > .5 ? part(a,rigidFrom,rigidBoundsFrom,rigidMaskFrom) : 1.;
  float alphaB = rigidMode > 1.5 && rigidCutout > .5 ? part(b,rigidTo,rigidBoundsTo,rigidMaskTo) : 1.;
  vec4 sampledA = sprite(a,fromPose), sampledB = sprite(b,toPose);
  if (rigidMode > 1.5 && rigidOpaque > .5) {
    float covered = wa * alphaA * sampledA.a + wb * alphaB * sampledB.a;
    float coverage = max(alphaA * sampledA.a,alphaB * sampledB.a);
    gl_FragColor = (sampledA * wa * alphaA + sampledB * wb * alphaB) / max(covered,.000001) * coverage;
  } else gl_FragColor = (sampledA * wa * alphaA + sampledB * wb * alphaB) / (wa + wb);
  if (rigidMode > 1.5) gl_FragColor *= targetCard;
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
    const textureLimit = gl.getParameter(gl.MAX_TEXTURE_SIZE) as number;
    if ([art, vectors, visibility].some(image => image.width > textureLimit || image.height > textureLimit)) throw new Error(`Mascot data exceeds GPU texture limit ${textureLimit}`);
    [art, vectors, visibility].forEach((image, index) => {
      const texture = gl.createTexture();
      if (!texture) throw new Error("Cannot allocate mascot texture");
      resources.textures.push(texture); gl.activeTexture(gl.TEXTURE0 + index); gl.bindTexture(gl.TEXTURE_2D, texture);
      gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, index === 0);
      gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      const format = index === 2 && flow.manifest.visibilityChannels !== 4 ? gl.LUMINANCE : gl.RGBA;
      gl.texImage2D(gl.TEXTURE_2D, 0, format, format, gl.UNSIGNED_BYTE, image);
      gl.uniform1i(uniform(index === 0 ? "artwork" : index === 1 ? "displacement" : "visibility"), index);
    });
    const manifest = flow.manifest;
    const temporal = manifest.algorithm === "rife4.25-keyframe-paths-v1";
    if (!temporal && manifest.algorithm !== "rife4.25-midpoint-backward") throw new Error("Unsupported mascot displacement data");
    if (temporal && (!manifest.transitions || manifest.transitions.layouts.length !== 8 || manifest.transitions.samples.length < 2
      || manifest.transitions.samples[0] !== 0 || manifest.transitions.samples[manifest.transitions.samples.length - 1] !== 1
      || manifest.transitions.samples.some((sample, i, all) => !Number.isFinite(sample) || (i > 0 && sample <= all[i - 1])))) throw new Error("Invalid mascot pose paths");
    const hd = !!flow.actions;
    if ((hd ? art.width !== art.height || art.width < 768 : art.width / 4 !== art.height / 8) || vectors.width !== manifest.width || vectors.height !== manifest.height || visibility.width !== manifest.width || visibility.height !== manifest.height) throw new Error("Invalid mascot texture dimensions");
    if (gl.getError() !== gl.NO_ERROR) throw new Error("Mascot texture upload failed");
    gl.uniform2f(uniform("atlasSize"), art.width, art.height); gl.uniform2f(uniform("mapSize"), vectors.width, vectors.height);
    gl.uniform1f(uniform("tile"), manifest.tile);
    gl.uniform1f(uniform("temporal"), temporal ? 1 : 0); gl.uniform2f(uniform("frameGrid"),2,1);
    gl.uniform1f(uniform("matteGreen"), manifest.visibilityChannels === 4 ? 1 : 0);
    gl.uniform1f(uniform("hd"), hd ? 1 : 0); gl.uniform2f(uniform("restSize"), art.width, art.height); gl.uniform1i(uniform("restArtwork"), 0);
    if (flow.actions) actions = new SpriteMascotActionTextures(gl, flow.actions, manifest.transitions?.layouts);
    const locations = ["fromPose", "toPose", "progress", "flowScale", "reverseFlow", "mapOrigin"].map(uniform);
    const artLocation = uniform("artwork"), sizeLocation = uniform("atlasSize");
    const nextMapLocation = uniform("nextMapOrigin"), fieldMixLocation = uniform("fieldMix"), gridLocation = uniform("frameGrid");
    const rigidLocations = ["rigidMode", "rigidFrom", "rigidTo", "rigidTarget", "rigidBoundsFrom", "rigidBoundsTo", "rigidBoundsTarget"].map(uniform);
    const cutoutLocation = uniform("rigidCutout"), opaqueLocation = uniform("rigidOpaque"), maskLocations = [uniform("rigidMaskFrom"), uniform("rigidMaskTo")];
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
        const transition = temporal ? spriteMascotTransition(pose, manifest.transitions!) : null;
        const entry = manifest.pairs[transition?.fieldKey ?? `${Math.min(pose.from, pose.to)}:${Math.max(pose.from, pose.to)}`];
        if ((transition ? transition.from !== transition.to : pose.from !== pose.to) && !entry) throw new Error(`Missing mascot displacement ${pose.from}:${pose.to}`);
        const [firstIndex, scale] = entry ?? [0, 0], stride = manifest.tile + manifest.padding * 2;
        const index = firstIndex + (transition?.sample ?? 0);
        const from = transition?.from ?? (hd ? pose.from % 4 : pose.from), to = transition?.to ?? (hd ? pose.to % 4 : pose.to);
        gl.uniform1f(locations[0], from); gl.uniform1f(locations[1], to); gl.uniform1f(locations[2], transition?.mix ?? pose.mix);
        gl.uniform1f(locations[3], scale); gl.uniform1f(locations[4], from > to ? 1 : 0);
        gl.uniform2f(locations[5], (index % manifest.columns) * stride + manifest.padding, Math.floor(index / manifest.columns) * stride + manifest.padding);
        if (transition) {
          gl.uniform2f(nextMapLocation, ((index + 1) % manifest.columns) * stride + manifest.padding, Math.floor((index + 1) / manifest.columns) * stride + manifest.padding);
          gl.uniform1f(fieldMixLocation, transition.sampleMix); gl.uniform2f(gridLocation, transition.layout.columns, transition.layout.rows);
        }
        gl.uniform1f(rigidLocations[0], 0);
        gl.uniform1f(cutoutLocation, 0);
        gl.uniform1f(opaqueLocation, 0);
        const rigidA = transition && manifest.rigidGuides?.[`${transition.row}:${from}`];
        const rigidB = transition && manifest.rigidGuides?.[`${transition.row}:${to}`];
        const layered = entry && entry.length === 4 && transition && transition.mix > 0 && transition.mix < 1 && rigidA && rigidB;
        if (layered) {
          const t = transition.mix;
          const target = rigidA.word.map((point, i) => point.map((value, j) => value * (1 - t) + rigidB.word[i][j] * t));
          gl.uniformMatrix3fv(rigidLocations[1], false, spriteMascotRigidTransform(rigidA.word));
          gl.uniformMatrix3fv(rigidLocations[2], false, spriteMascotRigidTransform(rigidB.word));
          gl.uniformMatrix3fv(rigidLocations[3], false, spriteMascotRigidTransform(target));
          gl.uniform4fv(rigidLocations[4], rigidA.bounds); gl.uniform4fv(rigidLocations[5], rigidB.bounds);
          gl.uniform4fv(rigidLocations[6], rigidA.bounds.map((v, i) => v * (1 - t) + rigidB.bounds[i] * t));
          gl.uniform1f(rigidLocations[0], 1);
          gl.uniform1f(opaqueLocation, rigidA.kind === "card" && rigidB.kind === "card" ? 1 : 0);
          if (rigidA.maskTile !== undefined && rigidB.maskTile !== undefined) {
            gl.uniform1f(cutoutLocation, 1);
            [rigidA, rigidB].forEach((guide, i) => {
              if (guide.maskTile === undefined) throw new Error("Missing mascot part matte");
              gl.uniform2f(maskLocations[i], (guide.maskTile % manifest.columns) * stride + manifest.padding, Math.floor(guide.maskTile / manifest.columns) * stride + manifest.padding);
            });
          }
        }
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        if (layered) {
          const propIndex = entry[2] + transition.sample;
          gl.uniform1f(rigidLocations[0], 2); gl.uniform1f(locations[3], entry[3]);
          gl.uniform2f(locations[5], (propIndex % manifest.columns) * stride + manifest.padding, Math.floor(propIndex / manifest.columns) * stride + manifest.padding);
          gl.uniform2f(nextMapLocation, ((propIndex + 1) % manifest.columns) * stride + manifest.padding, Math.floor((propIndex + 1) / manifest.columns) * stride + manifest.padding);
          gl.enable(gl.BLEND); gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
          gl.drawArrays(gl.TRIANGLES, 0, 6);
          gl.disable(gl.BLEND);
        }
        return true;
      },
      dispose,
    };
  } catch (error) { dispose(); throw error; }
}
