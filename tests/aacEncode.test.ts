/**
 * The software AAC encoder (used when the browser has no AAC WebCodecs) must produce
 * AAC-LC that decodes to the tone we put in, not silence. QuickTime plays this and
 * does not play Opus-in-MP4.
 */
import { describe, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { SoftwareAacEncoder } from "../src/ui/aacEncode.ts";

describe("software AAC", () => {
  test("a tone comes back audible as AAC-LC", async () => {
    const sr = 48000;
    const n = sr;
    const pcm = new Float32Array(n * 2);
    for (let i = 0; i < n; i++) {
      const s = Math.sin((2 * Math.PI * 440 * i) / sr) * 0.25;
      pcm[i * 2] = s;
      pcm[i * 2 + 1] = s;
    }
    const enc = await SoftwareAacEncoder.create(sr, 2);
    const packets = [...enc.push(pcm, 0), ...enc.flush()];
    const config = enc.config;
    enc.close();
    expect(config.length).toBeGreaterThanOrEqual(2);
    expect(packets.length).toBeGreaterThan(10);

    const ffmpeg = spawnSync("ffmpeg", ["-version"]);
    if (ffmpeg.status !== 0) return;
    const bytes: Uint8Array[] = [];
    for (const p of packets) bytes.push(adts(config, p.data.length), p.data);
    const file = new Uint8Array(bytes.reduce((s, b) => s + b.length, 0));
    let o = 0;
    for (const b of bytes) {
      file.set(b, o);
      o += b.length;
    }
    const { writeFileSync } = await import("node:fs");
    writeFileSync("/tmp/chimera-aac-tone.aac", file);
    const info = spawnSync("ffprobe", ["-v", "error", "-show_entries", "stream=codec_name,profile,sample_rate,channels", "-of", "default=nw=1", "/tmp/chimera-aac-tone.aac"], { encoding: "utf8" });
    expect(info.stdout).toContain("codec_name=aac");
    expect(info.stdout).toContain("profile=LC");
    const loud = spawnSync("ffmpeg", ["-hide_banner", "-i", "/tmp/chimera-aac-tone.aac", "-af", "volumedetect", "-f", "null", "-"], { encoding: "utf8" });
    const mean = /mean_volume:\s*(-?\d+(?:\.\d+)?) dB/.exec(loud.stderr);
    expect(mean, loud.stderr).not.toBeNull();
    expect(Number(mean![1])).toBeGreaterThan(-40);
  });
});

function adts(asc: Uint8Array, frameLen: number): Uint8Array {
  const profile = ((asc[0] >> 3) & 0x1f) - 1;
  const sri = ((asc[0] & 7) << 1) | (asc[1] >> 7);
  const ch = (asc[1] >> 3) & 0xf;
  const len = frameLen + 7;
  const h = new Uint8Array(7);
  h[0] = 0xff;
  h[1] = 0xf1;
  h[2] = ((profile & 3) << 6) | ((sri & 0xf) << 2) | ((ch >> 2) & 1);
  h[3] = ((ch & 3) << 6) | ((len >> 11) & 0x3);
  h[4] = (len >> 3) & 0xff;
  h[5] = ((len & 7) << 5) | 0x1f;
  h[6] = 0xfc;
  return h;
}
