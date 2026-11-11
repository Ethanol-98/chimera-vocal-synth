/**
 * One H.264 video track and one AAC audio track into a plain MP4 (ftyp + moov + mdat).
 *
 * WebCodecs hands us length-prefixed NALs plus an avcC record, and raw AAC frames plus an
 * AudioSpecificConfig. The timestamps are microseconds on one clock. A track that starts later
 * than the other gets an empty edit, so the picture stays with the sound.
 */

export interface Mp4VideoSample {
  data: Uint8Array;
  /** microseconds */
  timestamp: number;
  key: boolean;
}

export interface Mp4AudioSample {
  data: Uint8Array;
  timestamp: number;
}

export interface MuxInput {
  width: number;
  height: number;
  /** AVCDecoderConfigurationRecord, the bytes inside the avcC box */
  avcC: Uint8Array;
  /** AAC: AudioSpecificConfig. Opus: the OpusHead packet from the encoder. */
  audioConfig: Uint8Array;
  audioCodec: "aac" | "opus";
  sampleRate: number;
  channels: number;
  video: Mp4VideoSample[];
  audio: Mp4AudioSample[];
}

const MOVIE_TIMESCALE = 1000;

export function muxMp4(input: MuxInput): Uint8Array {
  if (!input.video.length || !input.audio.length) throw new Error("An MP4 needs both picture and sound");
  if (input.width < 2 || input.height < 2) throw new Error("The picture is empty");
  const video = shift(input.video);
  const audio = shift(input.audio);
  const t0 = Math.min(video[0].timestamp, audio[0].timestamp);
  for (const s of video) s.timestamp -= t0;
  for (const a of audio) a.timestamp -= t0;
  const videoDur = durations(video.map((s) => s.timestamp), 1_000_000);
  const audioDur = durations(audio.map((s) => s.timestamp), input.sampleRate);
  const videoBytes = video.reduce((n, s) => n + s.data.byteLength, 0);
  const audioBytes = audio.reduce((n, s) => n + s.data.byteLength, 0);

  const brands = [fourcc("isom"), fourcc("iso2"), fourcc("avc1"), fourcc("mp41")];
  if (input.audioCodec === "opus") brands.push(fourcc("Opus"));
  const ftyp = box("ftyp", fourcc("isom"), u32(0x200), ...brands);
  const build = (dataStart: number) => {
    const vOff = dataStart;
    const aOff = dataStart + videoBytes;
    const vTrack = trak({
      id: 1,
      video: true,
      width: input.width,
      height: input.height,
      timescale: 1_000_000,
      delayUs: video[0].timestamp,
      durations: videoDur,
      sizes: video.map((s) => s.data.byteLength),
      keys: video.map((s, i) => (s.key ? i + 1 : 0)).filter((n) => n > 0),
      chunkOffset: vOff,
      sampleEntry: avc1(input.width, input.height, input.avcC),
    });
    const aTrack = trak({
      id: 2,
      video: false,
      width: 0,
      height: 0,
      timescale: input.sampleRate,
      delayUs: audio[0].timestamp,
      durations: audioDur,
      sizes: audio.map((s) => s.data.byteLength),
      keys: [],
      chunkOffset: aOff,
      sampleEntry: input.audioCodec === "opus" ? opusEntry(input.sampleRate, input.channels, input.audioConfig) : mp4a(input.sampleRate, input.channels, input.audioConfig),
    });
    const movieUs = Math.max(video[0].timestamp + sum(videoDur.map((d) => d.delta * d.count)) , audio[0].timestamp + Math.round((sum(audioDur.map((d) => d.delta * d.count)) * 1_000_000) / input.sampleRate));
    return box("moov", mvhd(movieUs), vTrack, aTrack);
  };
  const probe = build(0);
  const dataStart = ftyp.byteLength + probe.byteLength + 8;
  const moov = build(dataStart);
  if (moov.byteLength !== probe.byteLength) throw new Error("MP4 layout changed size when the offsets were filled in");
  const mdat = box("mdat", ...video.map((s) => s.data), ...audio.map((s) => s.data));
  if (mdat.byteLength !== 8 + videoBytes + audioBytes) throw new Error("MP4 sound and picture did not fit the sample table");
  return concat(ftyp, moov, mdat);
}

function shift<T extends { timestamp: number }>(samples: T[]): T[] {
  return [...samples].sort((a, b) => a.timestamp - b.timestamp);
}

interface Run {
  count: number;
  delta: number;
}

/** Sample durations in `timescale` units, from timestamp gaps (microseconds in, except the last). */
function durations(timestampsUs: number[], timescale: number): Run[] {
  const deltas: number[] = [];
  for (let i = 0; i < timestampsUs.length; i++) {
    const next = i + 1 < timestampsUs.length ? timestampsUs[i + 1] : timestampsUs[i] + (i > 0 ? timestampsUs[i] - timestampsUs[i - 1] : 1_000_000 / 30);
    const us = Math.max(1, next - timestampsUs[i]);
    deltas.push(Math.max(1, Math.round((us * timescale) / 1_000_000)));
  }
  const runs: Run[] = [];
  for (const d of deltas) {
    const last = runs[runs.length - 1];
    if (last && last.delta === d) last.count++;
    else runs.push({ count: 1, delta: d });
  }
  return runs;
}

function sum(xs: number[]): number {
  return xs.reduce((a, b) => a + b, 0);
}

function trak(o: { id: number; video: boolean; width: number; height: number; timescale: number; delayUs: number; durations: Run[]; sizes: number[]; keys: number[]; chunkOffset: number; sampleEntry: Uint8Array }): Uint8Array {
  const mediaUnits = sum(o.durations.map((d) => d.count * d.delta));
  const mediaUs = o.timescale === 1_000_000 ? mediaUnits : Math.round((mediaUnits * 1_000_000) / o.timescale);
  const delay = o.delayUs > 1500 ? Math.round((o.delayUs * MOVIE_TIMESCALE) / 1_000_000) : 0;
  const present = Math.max(1, Math.round((mediaUs * MOVIE_TIMESCALE) / 1_000_000));
  const tkhdDur = present + delay;
  const table = [stsd(o.sampleEntry), stts(o.durations)];
  if (o.video) table.push(stss(o.keys));
  table.push(stsc(o.sizes.length), stsz(o.sizes), stco(o.chunkOffset));
  const kids = [tkhd(o.id, tkhdDur, o.video, o.width, o.height)];
  if (delay > 0) kids.push(box("edts", elst(delay, present)));
  kids.push(box("mdia", mdhd(o.timescale, mediaUnits), hdlr(o.video ? "vide" : "soun", o.video ? "VideoHandler" : "SoundHandler"), box("minf", o.video ? vmhd() : smhd(), dinf(), box("stbl", ...table))));
  return box("trak", ...kids);
}

function mvhd(durationUs: number): Uint8Array {
  const dur = Math.max(1, Math.round((durationUs * MOVIE_TIMESCALE) / 1_000_000));
  return box(
    "mvhd",
    u32(0),
    u32(0),
    u32(0),
    u32(MOVIE_TIMESCALE),
    u32(dur),
    u32(0x00010000),
    u16(0x0100),
    zeros(10),
    matrix(),
    zeros(24),
    u32(3),
  );
}

function tkhd(id: number, duration: number, video: boolean, width: number, height: number): Uint8Array {
  return box(
    "tkhd",
    u32(0x7),
    u32(0),
    u32(0),
    u32(id),
    u32(0),
    u32(duration),
    zeros(8),
    u16(0),
    u16(0),
    u16(video ? 0 : 0x0100),
    u16(0),
    matrix(),
    u32(width << 16),
    u32(height << 16),
  );
}

function elst(emptyDur: number, mediaDur: number): Uint8Array {
  return box("elst", u32(0), u32(2), u32(emptyDur), i32(-1), u16(1), u16(0), u32(mediaDur), i32(0), u16(1), u16(0));
}

function mdhd(timescale: number, duration: number): Uint8Array {
  return box("mdhd", u32(0), u32(0), u32(0), u32(timescale), u32(duration), u16(0x55c4), u16(0));
}

function hdlr(type: string, name: string): Uint8Array {
  return box("hdlr", u32(0), u32(0), fourcc(type), zeros(12), utf8(name + "\0"));
}

function vmhd(): Uint8Array {
  return box("vmhd", u32(1), u16(0), zeros(6));
}

function smhd(): Uint8Array {
  return box("smhd", u32(0), u16(0), u16(0));
}

function dinf(): Uint8Array {
  return box("dinf", box("dref", u32(0), u32(1), box("url ", u32(1))));
}

function stsd(entry: Uint8Array): Uint8Array {
  return box("stsd", u32(0), u32(1), entry);
}

function stts(runs: Run[]): Uint8Array {
  return box("stts", u32(0), u32(runs.length), ...runs.flatMap((r) => [u32(r.count), u32(r.delta)]));
}

function stss(keys: number[]): Uint8Array {
  const list = keys.length ? keys : [1];
  return box("stss", u32(0), u32(list.length), ...list.map(u32));
}

function stsc(sampleCount: number): Uint8Array {
  return box("stsc", u32(0), u32(1), u32(1), u32(sampleCount), u32(1));
}

function stsz(sizes: number[]): Uint8Array {
  return box("stsz", u32(0), u32(0), u32(sizes.length), ...sizes.map(u32));
}

function stco(offset: number): Uint8Array {
  return box("stco", u32(0), u32(1), u32(offset));
}

function avc1(width: number, height: number, avcC: Uint8Array): Uint8Array {
  const name = new Uint8Array(32);
  const label = utf8("AVC Coding");
  name[0] = label.byteLength;
  name.set(label, 1);
  return box(
    "avc1",
    zeros(6),
    u16(1),
    u16(0),
    u16(0),
    zeros(12),
    u16(width),
    u16(height),
    u32(0x00480000),
    u32(0x00480000),
    u32(0),
    u16(1),
    name,
    u16(0x0018),
    i16(-1),
    box("avcC", avcC),
  );
}

function mp4a(sampleRate: number, channels: number, config: Uint8Array): Uint8Array {
  return box("mp4a", zeros(6), u16(1), zeros(8), u16(channels), u16(16), u16(0), u16(0), u32(sampleRate << 16), esds(config));
}

/** Opus in an MP4 (dOps), for browsers that can encode H.264 but not AAC. */
function opusEntry(sampleRate: number, channels: number, head: Uint8Array): Uint8Array {
  // OpusHead is little-endian. dOps is the same fields, big-endian, without the magic.
  const view = new DataView(head.buffer, head.byteOffset, head.byteLength);
  const magic = new TextDecoder().decode(head.subarray(0, 8));
  const ch = magic === "OpusHead" && head.byteLength >= 19 ? head[9] : channels;
  const preSkip = magic === "OpusHead" && head.byteLength >= 19 ? view.getUint16(10, true) : 3840;
  const rate = magic === "OpusHead" && head.byteLength >= 19 ? view.getUint32(12, true) : sampleRate;
  const gain = magic === "OpusHead" && head.byteLength >= 19 ? view.getInt16(16, true) : 0;
  const family = magic === "OpusHead" && head.byteLength >= 19 ? head[18] : 0;
  const dops = box("dOps", u8(0), u8(ch), u16(preSkip), u32(rate), i16(gain), u8(family));
  return box("Opus", zeros(6), u16(1), zeros(8), u16(ch), u16(16), u16(0), u16(0), u32(48000 << 16), dops);
}

function esds(config: Uint8Array): Uint8Array {
  const dsi = desc(0x05, config);
  const dec = desc(0x04, concat(u8(0x40), u8(0x15), u8(0), u8(0), u8(0), u32(128000), u32(128000), dsi));
  const sl = desc(0x06, u8(0x02));
  const es = desc(0x03, concat(u16(1), u8(0), dec, sl));
  return box("esds", u32(0), es);
}

function desc(tag: number, payload: Uint8Array): Uint8Array {
  // One-byte expandable length. AAC's AudioSpecificConfig is two bytes; anything we write fits.
  if (payload.byteLength >= 128) throw new Error("MP4 descriptor too long");
  return concat(u8(tag), u8(payload.byteLength), payload);
}

function matrix(): Uint8Array {
  return concat(u32(0x00010000), u32(0), u32(0), u32(0), u32(0x00010000), u32(0), u32(0), u32(0), u32(0x40000000));
}

function box(type: string, ...parts: Uint8Array[]): Uint8Array {
  const body = concat(...parts);
  const out = new Uint8Array(8 + body.byteLength);
  const view = new DataView(out.buffer);
  view.setUint32(0, out.byteLength);
  out.set(fourcc(type), 4);
  out.set(body, 8);
  return out;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const n = parts.reduce((s, p) => s + p.byteLength, 0);
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.byteLength;
  }
  return out;
}

function fourcc(s: string): Uint8Array {
  return utf8(s);
}

function utf8(s: string): Uint8Array {
  return new TextEncoder().encode(s);
}

function u32(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n >>> 0);
  return b;
}

function i32(n: number): Uint8Array {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setInt32(0, n);
  return b;
}

function u16(n: number): Uint8Array {
  const b = new Uint8Array(2);
  new DataView(b.buffer).setUint16(0, n);
  return b;
}

function i16(n: number): Uint8Array {
  const b = new Uint8Array(2);
  new DataView(b.buffer).setInt16(0, n);
  return b;
}

function u8(n = 0): Uint8Array {
  return new Uint8Array([n]);
}

function zeros(n: number): Uint8Array {
  return new Uint8Array(n);
}
