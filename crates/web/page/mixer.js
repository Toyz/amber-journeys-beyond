// The audio worklet. It cannot reach the game -- that is on the main thread --
// so it asks for a buffer and plays whatever comes back, keeping one ahead so
// a late reply is silence rather than a gap.
class AmberMixer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.port.onmessage = ({ data }) => this.queue.push(data);
    this.port.postMessage({ frames: 128 });
  }

  process(_inputs, outputs) {
    const [left, right] = outputs[0];
    const buf = this.queue.shift();
    if (buf) {
      for (let i = 0; i < left.length; i++) {
        left[i] = buf[i * 2];
        right[i] = buf[i * 2 + 1];
      }
    } else {
      left.fill(0);
      right.fill(0);
    }
    this.port.postMessage({ frames: left.length });
    return true;
  }
}
registerProcessor("amber-mixer", AmberMixer);
