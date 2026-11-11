/**
 * A copy of whatever is playing, for an MP4 export. Connected only while recording, and not
 * the voice's own processor: the engine's audio thread is unchanged.
 *
 * Each block is posted with the context sample it starts on (`currentFrame`), so the main
 * thread can stamp it on the same clock as the picture.
 */
declare const currentFrame: number;
declare function registerProcessor(name: string, ctor: unknown): void;
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
}

class RecorderProcessor extends AudioWorkletProcessor {
  process(inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    // A node with no output is not always pulled (Safari). The output is silence into a
    // gain of 0, so it doesn't add to what the speakers play.
    for (const ch of outputs[0] ?? []) ch.fill(0);
    const ch0 = inputs[0]?.[0];
    if (!ch0 || ch0.length === 0) return true;
    const ch1 = inputs[0][1] ?? ch0;
    const a = new Float32Array(ch0);
    const b = new Float32Array(ch1);
    this.port.postMessage({ frame: currentFrame, ch0: a, ch1: b }, [a.buffer, b.buffer]);
    return true;
  }
}

registerProcessor("chimera-recorder", RecorderProcessor);
