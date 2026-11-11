/**
 * The MP4 writer used by Render MP4: a real ftyp/moov/mdat file, H.264 + AAC sample tables,
 * and a later picture track held with an empty edit so it stays in line with the sound.
 */
import { describe, expect, test } from "bun:test";
import { muxMp4 } from "../src/ui/mp4mux.ts";

function ascii(data: Uint8Array, at: number, n: number): string {
  return String.fromCharCode(...data.subarray(at, at + n));
}

function boxes(data: Uint8Array, start = 0, end = data.length): { type: string; start: number; size: number }[] {
  const out: { type: string; start: number; size: number }[] = [];
  let at = start;
  while (at + 8 <= end) {
    const size = new DataView(data.buffer, data.byteOffset + at, 4).getUint32(0);
    const type = ascii(data, at + 4, 4);
    if (size < 8 || at + size > end) break;
    out.push({ type, start: at, size });
    at += size;
  }
  return out;
}

describe("muxMp4", () => {
  const avcC = new Uint8Array([1, 0x42, 0, 0x1f, 0xff, 0xe1, 0, 3, 0x67, 0x42, 0, 1, 0, 1, 0x68, 0xce]);
  const audioConfig = new Uint8Array([0x12, 0x10]);
  const video = [
    { data: new Uint8Array([0, 0, 0, 1, 0x65]), timestamp: 0, key: true },
    { data: new Uint8Array([0, 0, 0, 2, 0x41, 0x9a]), timestamp: 33333, key: false },
    { data: new Uint8Array([0, 0, 0, 2, 0x41, 0x11]), timestamp: 66666, key: false },
  ];
  const audio = [
    { data: new Uint8Array([0x21, 0x10, 0x04, 0x60]), timestamp: 0 },
    { data: new Uint8Array([0x21, 0x10, 0x04, 0x61]), timestamp: 21333 },
    { data: new Uint8Array([0x21, 0x10, 0x04, 0x62]), timestamp: 42666 },
  ];

  test("writes a self-contained MP4 whose sample tables point at the picture and the sound", () => {
    const file = muxMp4({ width: 640, height: 360, avcC, audioConfig, audioCodec: "aac", sampleRate: 48000, channels: 2, video, audio });
    const top = boxes(file);
    expect(top.map((b) => b.type)).toEqual(["ftyp", "moov", "mdat"]);
    expect(top.reduce((n, b) => n + b.size, 0)).toBe(file.byteLength);
    expect(ascii(file, 8, 4)).toBe("isom");

    const payload = file.subarray(top[2].start + 8, top[2].start + top[2].size);
    const samples = [...video.map((s) => s.data), ...audio.map((s) => s.data)];
    expect(payload.byteLength).toBe(samples.reduce((n, s) => n + s.byteLength, 0));
    let o = 0;
    for (const s of samples) {
      expect(payload.subarray(o, o + s.byteLength)).toEqual(s);
      o += s.byteLength;
    }

    const text = new TextDecoder("latin1").decode(file);
    for (const mark of ["avc1", "avcC", "mp4a", "esds", "stsz", "stco"]) expect(text).toContain(mark);

    const stszAt = text.indexOf("stsz");
    const view = new DataView(file.buffer, file.byteOffset + stszAt + 4);
    // version/flags, sample_size, sample_count
    expect(view.getUint32(8)).toBe(video.length);
  });

  test("a picture that starts later still shares the sound's clock", () => {
    const late = video.map((s, i) => ({ ...s, timestamp: s.timestamp + 40_000, data: new Uint8Array([i + 1]) }));
    const file = muxMp4({ width: 320, height: 180, avcC, audioConfig, audioCodec: "aac", sampleRate: 48000, channels: 2, video: late, audio });
    expect(new TextDecoder("latin1").decode(file)).toContain("elst");
    expect(boxes(file).map((b) => b.type)).toEqual(["ftyp", "moov", "mdat"]);
  });

  test("opus sits in the same MP4 when the browser has no AAC encoder", () => {
    const head = new Uint8Array(19);
    head.set(new TextEncoder().encode("OpusHead"));
    head[8] = 1;
    head[9] = 2;
    new DataView(head.buffer).setUint16(10, 312, true);
    new DataView(head.buffer).setUint32(12, 48000, true);
    const file = muxMp4({ width: 320, height: 180, avcC, audioConfig: head, audioCodec: "opus", sampleRate: 48000, channels: 2, video, audio });
    const text = new TextDecoder("latin1").decode(file);
    expect(text).toContain("Opus");
    expect(text).toContain("dOps");
    expect(text).toContain("avc1");
    expect(boxes(file).map((b) => b.type)).toEqual(["ftyp", "moov", "mdat"]);
  });

  test("refuses a recording with no picture or no sound", () => {
    expect(() => muxMp4({ width: 320, height: 180, avcC, audioConfig, audioCodec: "aac", sampleRate: 48000, channels: 2, video: [], audio })).toThrow(/picture and sound/);
  });
});
