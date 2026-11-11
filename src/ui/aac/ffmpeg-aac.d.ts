/** FFmpeg's AAC encoder, compiled to wasm. See `COPYING.LGPLv2.1` beside this file. */
interface AacWasmModule {
  cwrap: (name: string, ret: string | null, args: string[]) => (...args: never[]) => unknown;
  HEAPU8: Uint8Array;
}

export default function create(arg?: object): Promise<AacWasmModule>;
