/**
 * AAC-LC frames for the MP4, when the browser's WebCodecs has no AAC encoder.
 *
 * The wasm module is FFmpeg's native AAC encoder (LGPL-2.1, `src/ui/aac/COPYING.LGPLv2.1`
 * and `FFMPEG-LICENSE.md`), built by the Mediabunny project (MPL-2.0). It is loaded only
 * for an export, not with the page. The bytes it returns are raw AAC access units plus the
 * AudioSpecificConfig that goes in the mp4a sample entry. QuickTime, Safari, and Windows
 * Media Player play that and do not play Opus inside an MP4.
 */

type AacModule = {
  cwrap: (name: string, ret: string | null, args: string[]) => (...args: never[]) => unknown;
  HEAPU8: Uint8Array;
};

export interface AacPacket {
  data: Uint8Array;
  /** microseconds, on the same clock as the video frames */
  timestamp: number;
}

let modulePromise: Promise<AacModule> | null = null;

function loadModule(): Promise<AacModule> {
  if (!modulePromise) {
    modulePromise = import("./aac/ffmpeg-aac.js").then((m: { default: (arg?: object) => Promise<AacModule> }) => m.default());
  }
  return modulePromise;
}

export class SoftwareAacEncoder {
  private constructor(
    private mod: AacModule,
    private ctx: number,
    readonly frameSize: number,
    readonly config: Uint8Array,
    readonly sampleRate: number,
    readonly channels: number,
  ) {}

  static async create(sampleRate: number, channels: number, bitrate = 128000): Promise<SoftwareAacEncoder> {
    const mod = await loadModule();
    const init = mod.cwrap("init_encoder", "number", ["number", "number", "number"]) as (ch: number, rate: number, br: number) => number;
    const frameSizeOf = mod.cwrap("get_encoder_frame_size", "number", ["number"]) as (ctx: number) => number;
    const extraPtr = mod.cwrap("get_encoder_extradata", "number", ["number"]) as (ctx: number) => number;
    const extraSize = mod.cwrap("get_encoder_extradata_size", "number", ["number"]) as (ctx: number) => number;
    const ctx = init(channels, sampleRate, bitrate);
    if (!ctx) throw new Error("The AAC encoder didn't start");
    const size = extraSize(ctx);
    const config = mod.HEAPU8.slice(extraPtr(ctx), extraPtr(ctx) + size);
    return new SoftwareAacEncoder(mod, ctx, frameSizeOf(ctx), config, sampleRate, channels);
  }

  private rest = new Float32Array(0);
  private restSample = 0;

  /** Interleaved float samples. `firstSample` is the index of the first one, on the audio-context clock. */
  push(interleaved: Float32Array, firstSample: number): AacPacket[] {
    if (this.rest.length === 0) this.restSample = firstSample;
    const joined = new Float32Array(this.rest.length + interleaved.length);
    joined.set(this.rest, 0);
    joined.set(interleaved, this.rest.length);
    const frameSamples = this.frameSize * this.channels;
    const full = Math.floor(joined.length / frameSamples) * frameSamples;
    const packets = full > 0 ? this.encodeExact(joined.subarray(0, full), this.restSample) : [];
    this.rest = joined.slice(full);
    this.restSample += full / this.channels;
    return packets;
  }

  flush(): AacPacket[] {
    const packets: AacPacket[] = [];
    if (this.rest.length) {
      const padded = new Float32Array(this.frameSize * this.channels);
      padded.set(this.rest);
      packets.push(...this.encodeExact(padded, this.restSample));
      this.rest = new Float32Array(0);
    }
    const start = this.mod.cwrap("flush_encoder_start", null, ["number"]) as (ctx: number) => void;
    start(this.ctx);
    packets.push(...this.drain(this.sampleRate));
    return packets;
  }

  private encodeExact(interleaved: Float32Array, firstSample: number): AacPacket[] {
    const { mod, ctx, frameSize, channels, sampleRate } = this;
    const inputPtrOf = mod.cwrap("get_encode_input_ptr", "number", ["number", "number"]) as (ctx: number, size: number) => number;
    const send = mod.cwrap("send_frame", "number", ["number", "bigint"]) as (ctx: number, pts: bigint) => number;
    const out: AacPacket[] = [];
    const frameSamples = frameSize * channels;
    for (let offset = 0, sample = firstSample; offset + frameSamples <= interleaved.length; offset += frameSamples, sample += frameSize) {
      const slice = interleaved.subarray(offset, offset + frameSamples);
      const ptr = inputPtrOf(ctx, slice.byteLength);
      if (!ptr) throw new Error("The AAC encoder ran out of memory");
      mod.HEAPU8.set(new Uint8Array(slice.buffer, slice.byteOffset, slice.byteLength), ptr);
      const ret = send(ctx, BigInt(sample));
      if (ret < 0) throw new Error(`AAC encode failed (${ret})`);
      out.push(...this.drain(sampleRate));
    }
    return out;
  }

  close(): void {
    const close = this.mod.cwrap("close_encoder", null, ["number"]) as (ctx: number) => void;
    close(this.ctx);
  }

  private drain(sampleRate: number): AacPacket[] {
    const recv = this.mod.cwrap("receive_packet", "number", ["number"]) as (ctx: number) => number;
    const dataPtr = this.mod.cwrap("get_encoded_data", "number", ["number"]) as (ctx: number) => number;
    const ptsOf = this.mod.cwrap("get_encoded_pts", "bigint", ["number"]) as (ctx: number) => bigint;
    const out: AacPacket[] = [];
    for (;;) {
      const size = recv(this.ctx);
      if (size <= 0) break;
      const ptr = dataPtr(this.ctx);
      const data = this.mod.HEAPU8.slice(ptr, ptr + size);
      const pts = Number(ptsOf(this.ctx));
      out.push({ data, timestamp: Math.round((pts * 1_000_000) / sampleRate) });
    }
    return out;
  }
}
