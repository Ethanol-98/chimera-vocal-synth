/**
 * Record the picture on screen and the sound coming out of the speakers, then download an MP4.
 *
 * MediaRecorder's MP4 support depends on the browser, and a plain "video/mp4" recording is
 * often VP9 and Opus under an MP4 name. WebCodecs encodes H.264, plus AAC when the browser
 * has it, and {@link muxMp4} writes that into a real MP4. Opus is the sound when AAC isn't
 * available (still inside the MP4, next to H.264). MediaRecorder's own MP4 is the last resort.
 */
import recorderUrl from "../worklet/recorder.ts?worker&url";
import { engine } from "../audio/AudioEngine.ts";
import { muxMp4, type Mp4AudioSample, type Mp4VideoSample } from "./mp4mux.ts";
import { currentPlan, speakText } from "./speech.ts";
import { demoTake, playDemo } from "./singing.ts";
import type { BodyMix } from "../wasm/types.ts";

const FPS = 30;
const MAX_W = 1280;

export async function exportSpeak(o: { text: string; ipa: boolean; body: BodyMix; rate: number; f0: number }): Promise<void> {
  await recordTake("chimera-speak.mp4", async () => {
    const plan = await speakText(o.text, o);
    if (!plan) throw new Error("Nothing to say");
    const offset = await waitFor(4000, () => (currentPlan()?.id === plan.id ? currentPlan()?.offset ?? null : null));
    return offset + plan.plan.end + 0.25;
  });
}

export async function exportDemo(name: string, body: BodyMix, ready: () => Promise<void>): Promise<void> {
  const file = `chimera-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "song"}.mp4`;
  await recordTake(file, async () => {
    await playDemo(name, body, ready);
    const take = await waitFor(8000, () => {
      const d = demoTake();
      return d && d.offset != null ? d : null;
    });
    return take.offset! + take.end + 0.45;
  });
}

async function recordTake(filename: string, run: () => Promise<number>): Promise<void> {
  await engine.start();
  const ctx = engine.ctx;
  if (!ctx) throw new Error("The voice didn't start");
  const path = await encoderPath(ctx.sampleRate);
  if (path === "aac" || path === "opus") await recordWebCodecs(ctx, filename, run, path);
  else if (path === "mediarecorder") await recordMediaRecorder(ctx, filename, run);
  else throw new Error("This browser can't write an MP4. It needs H.264, and AAC or Opus.");
}

type Path = "aac" | "opus" | "mediarecorder" | "none";

async function encoderPath(sampleRate: number): Promise<Path> {
  if (typeof VideoEncoder !== "undefined" && typeof AudioEncoder !== "undefined") {
    try {
      const video = await VideoEncoder.isConfigSupported({ codec: "avc1.42001f", width: 640, height: 360, bitrate: 1_000_000, framerate: FPS, latencyMode: "realtime" });
      if (video.supported) {
        const aac = await AudioEncoder.isConfigSupported({ codec: "mp4a.40.2", numberOfChannels: 2, sampleRate, bitrate: 128000 });
        if (aac.supported) return "aac";
        // AAC is the usual MP4 sound, but plenty of browsers ship H.264 without an AAC encoder.
        const opus = await AudioEncoder.isConfigSupported({ codec: "opus", numberOfChannels: 2, sampleRate: 48000, bitrate: 128000 });
        if (opus.supported && sampleRate === 48000) return "opus";
      }
    } catch {
      // try the other path
    }
  }
  return mp4Mime() ? "mediarecorder" : "none";
}

function mp4Mime(): string | null {
  if (typeof MediaRecorder === "undefined") return null;
  for (const mime of ["video/mp4;codecs=avc1.42E01E,mp4a.40.2", "video/mp4;codecs=avc1,mp4a.40.2", "video/mp4"]) {
    if (MediaRecorder.isTypeSupported(mime)) return mime;
  }
  return null;
}

async function recordWebCodecs(ctx: AudioContext, filename: string, run: () => Promise<number>, audioCodec: "aac" | "opus"): Promise<void> {
  const canvas = performanceCanvas();
  const { width, height } = fit(canvas.width, canvas.height);
  const off = makeCanvas(width, height);
  const octx = off.getContext("2d");
  if (!octx) throw new Error("Couldn't draw the picture");

  let avcC: Uint8Array | null = null;
  let audioConfig: Uint8Array | null = null;
  const video: Mp4VideoSample[] = [];
  const audio: Mp4AudioSample[] = [];
  let vError: Error | null = null;
  let aError: Error | null = null;

  const venc = new VideoEncoder({
    output: (chunk, meta) => {
      if (meta?.decoderConfig?.description) avcC = bytes(meta.decoderConfig.description);
      const data = new Uint8Array(chunk.byteLength);
      chunk.copyTo(data);
      video.push({ data: toAvcc(data), timestamp: chunk.timestamp, key: chunk.type === "key" });
    },
    error: (e) => {
      vError = e;
    },
  });
  const aenc = new AudioEncoder({
    output: (chunk, meta) => {
      if (meta?.decoderConfig?.description) audioConfig = bytes(meta.decoderConfig.description);
      const data = new Uint8Array(chunk.byteLength);
      chunk.copyTo(data);
      audio.push({ data, timestamp: chunk.timestamp });
    },
    error: (e) => {
      aError = e;
    },
  });
  const videoConfig: VideoEncoderConfig = { codec: "avc1.42001f", width, height, bitrate: 2_000_000, framerate: FPS, latencyMode: "realtime", avc: { format: "avc" } };
  try {
    venc.configure(videoConfig);
  } catch {
    venc.configure({ codec: "avc1.42001f", width, height, bitrate: 2_000_000, framerate: FPS });
  }
  aenc.configure({ codec: audioCodec === "aac" ? "mp4a.40.2" : "opus", numberOfChannels: 2, sampleRate: ctx.sampleRate, bitrate: 128000 });

  if (!recorderModule) {
    await ctx.audioWorklet.addModule(recorderUrl);
    recorderModule = true;
  }
  const rec = new AudioWorkletNode(ctx, "chimera-recorder", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 2 });
  const origin = ctx.currentTime;
  let queued0: Float32Array[] = [];
  let queued1: Float32Array[] = [];
  let queued = 0;
  let batchFrame = 0;
  const take = (need: number) => {
    const a = new Float32Array(need);
    const b = new Float32Array(need);
    let o = 0;
    while (o < need && queued0.length) {
      const c0 = queued0[0];
      const c1 = queued1[0];
      const n = Math.min(need - o, c0.length);
      a.set(c0.subarray(0, n), o);
      b.set(c1.subarray(0, n), o);
      o += n;
      if (n === c0.length) {
        queued0.shift();
        queued1.shift();
      } else {
        queued0[0] = c0.subarray(n);
        queued1[0] = c1.subarray(n);
      }
    }
    return { a, b };
  };
  const encodeAudio = (n: number) => {
    const frame = batchFrame;
    const { a, b } = take(n);
    const data = new Float32Array(n * 2);
    data.set(a, 0);
    data.set(b, n);
    const timestamp = Math.max(0, Math.round((frame / ctx.sampleRate - origin) * 1e6));
    const audioData = new AudioData({ format: "f32-planar", sampleRate: ctx.sampleRate, numberOfFrames: n, numberOfChannels: 2, timestamp, data });
    aenc.encode(audioData);
    audioData.close();
    queued -= n;
    batchFrame = frame + n;
  };
  rec.port.onmessage = (e: MessageEvent<{ frame: number; ch0: Float32Array; ch1: Float32Array }>) => {
    const { frame, ch0, ch1 } = e.data;
    if (queued === 0) batchFrame = frame;
    queued0.push(ch0);
    queued1.push(ch1);
    queued += ch0.length;
    while (queued >= 1024) encodeAudio(1024);
  };
  const untap = engine.tapOutput(rec);

  let stopFrames = false;
  let sent = 0;
  const draw = () => {
    if (stopFrames) return;
    if (!vError && venc.encodeQueueSize < 8) {
      octx.drawImage(canvas, 0, 0, width, height);
      const timestamp = Math.max(0, Math.round((ctx.currentTime - origin) * 1e6));
      const frame = new VideoFrame(off, { timestamp, duration: Math.round(1e6 / FPS) });
      venc.encode(frame, { keyFrame: sent % (FPS * 2) === 0 });
      frame.close();
      sent++;
    }
    window.setTimeout(draw, 1000 / FPS);
  };
  draw();

  let failed: unknown = null;
  try {
    const endAt = await run();
    await waitUntil(() => {
      const t = engine.voiceClock();
      return t != null && t >= endAt;
    });
    await sleep(180);
  } catch (e) {
    failed = e;
  } finally {
    stopFrames = true;
    untap();
    rec.port.onmessage = null;
    rec.disconnect();
  }
  if (failed) throw failed;
  if (queued > 0) encodeAudio(queued);
  await venc.flush();
  await aenc.flush();
  venc.close();
  aenc.close();
  if (vError || aError) throw vError ?? aError;
  if (!avcC || !audioConfig) throw new Error("The encoder didn't describe the MP4 tracks");
  if (!video.length || !audio.length) throw new Error("The recording was empty");
  saveFile(muxMp4({ width, height, avcC, audioConfig, audioCodec, sampleRate: ctx.sampleRate, channels: 2, video, audio }), filename);
}

let recorderModule = false;

async function recordMediaRecorder(ctx: AudioContext, filename: string, run: () => Promise<number>): Promise<void> {
  const mime = mp4Mime();
  if (!mime) throw new Error("This browser can't write an MP4. It needs H.264 and AAC.");
  const canvas = performanceCanvas();
  const dest = ctx.createMediaStreamDestination();
  const untap = engine.tapOutput(dest);
  const stream = canvas.captureStream(FPS);
  for (const track of dest.stream.getAudioTracks()) stream.addTrack(track);
  const rec = new MediaRecorder(stream, { mimeType: mime, videoBitsPerSecond: 2_500_000, audioBitsPerSecond: 128000 });
  const chunks: Blob[] = [];
  rec.ondataavailable = (e) => {
    if (e.data.size) chunks.push(e.data);
  };
  const stopped = new Promise<void>((resolve, reject) => {
    rec.onstop = () => resolve();
    rec.onerror = () => reject(new Error("The recorder failed"));
  });
  rec.start(200);
  let failed: unknown = null;
  try {
    const endAt = await run();
    await waitUntil(() => {
      const t = engine.voiceClock();
      return t != null && t >= endAt;
    });
    await sleep(180);
  } catch (e) {
    failed = e;
  } finally {
    if (rec.state !== "inactive") rec.stop();
    await stopped.catch(() => undefined);
    untap();
    for (const track of stream.getTracks()) track.stop();
  }
  if (failed) throw failed;
  const blob = new Blob(chunks, { type: "video/mp4" });
  const head = new Uint8Array(await blob.slice(0, 12).arrayBuffer());
  if (String.fromCharCode(head[4], head[5], head[6], head[7]) !== "ftyp") throw new Error("The recording was not an MP4");
  saveFile(new Uint8Array(await blob.arrayBuffer()), filename);
}

function performanceCanvas(): HTMLCanvasElement {
  const canvas = document.querySelector(".anatomy canvas, canvas.creature-canvas");
  if (!(canvas instanceof HTMLCanvasElement) || canvas.width < 2 || canvas.height < 2) throw new Error("The picture isn't on screen yet");
  return canvas;
}

function fit(w: number, h: number): { width: number; height: number } {
  let width = w;
  let height = h;
  if (width > MAX_W) {
    height = Math.round((height * MAX_W) / width);
    width = MAX_W;
  }
  return { width: Math.max(2, width & ~1), height: Math.max(2, height & ~1) };
}

function makeCanvas(width: number, height: number): OffscreenCanvas | HTMLCanvasElement {
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(width, height);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
}

/** Length-prefixed NALs, which is what an avcC sample needs. Annex B (start codes) is converted.
 * A length of 256–511 is `00 00 01 xx`, the same bytes as a 3-byte start code, so a sample that
 * already has a plausible NAL header after its length is left alone. */
function toAvcc(data: Uint8Array): Uint8Array {
  if (data.length >= 5) {
    const len = new DataView(data.buffer, data.byteOffset, 4).getUint32(0);
    const nalType = data[4] & 0x1f;
    if (len >= 2 && len + 4 <= data.length && nalType >= 1 && nalType <= 12 && (data[4] & 0x80) === 0) return data;
  }
  if (!(data.length > 4 && data[0] === 0 && data[1] === 0 && (data[2] === 1 || (data[2] === 0 && data[3] === 1)))) return data;
  const nals: Uint8Array[] = [];
  let i = 0;
  const start = (at: number) => (data[at] === 0 && data[at + 1] === 0 && data[at + 2] === 1 ? 3 : data[at] === 0 && data[at + 1] === 0 && data[at + 2] === 0 && data[at + 3] === 1 ? 4 : 0);
  while (i < data.length) {
    const s = start(i);
    if (!s) break;
    i += s;
    let j = i;
    while (j < data.length - 3 && !start(j)) j++;
    if (j >= data.length - 3) j = data.length;
    nals.push(data.subarray(i, j));
    i = j;
  }
  const out = new Uint8Array(nals.reduce((n, nal) => n + 4 + nal.byteLength, 0));
  const view = new DataView(out.buffer);
  let o = 0;
  for (const nal of nals) {
    view.setUint32(o, nal.byteLength);
    out.set(nal, o + 4);
    o += 4 + nal.byteLength;
  }
  return out;
}

function bytes(src: AllowSharedBufferSource): Uint8Array {
  if (ArrayBuffer.isView(src)) return new Uint8Array(src.buffer, src.byteOffset, src.byteLength).slice();
  return new Uint8Array(src as ArrayBuffer).slice();
}

function saveFile(data: Uint8Array, filename: string): void {
  const blob = new Blob([data.slice()], { type: "video/mp4" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 15_000);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => window.setTimeout(r, ms));
}

async function waitFor<T>(ms: number, read: () => T | null): Promise<T> {
  const t0 = performance.now();
  for (;;) {
    const v = read();
    if (v != null) return v;
    if (performance.now() - t0 > ms) throw new Error("The voice didn't start");
    await new Promise((r) => requestAnimationFrame(() => r(undefined)));
  }
}

async function waitUntil(done: () => boolean): Promise<void> {
  const t0 = performance.now();
  for (;;) {
    if (done()) return;
    if (performance.now() - t0 > 90_000) throw new Error("The render took too long");
    await new Promise((r) => requestAnimationFrame(() => r(undefined)));
  }
}
