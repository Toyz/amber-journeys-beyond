// The audio worklet.
//
// It cannot reach the game -- that is on the main thread and this is not -- so
// the main thread pushes mixed samples ahead of time and this plays them out
// of a queue.
//
// The first version of this asked for a buffer each time it was called and
// waited for the answer. `process` runs every 128 frames, which at 48kHz is
// under three milliseconds, and a round trip through a main thread that is
// also drawing the game does not fit in that. The queue was empty almost
// every time and the result was silence. Pushing ahead is the only shape that
// works without a SharedArrayBuffer, which would need the page to be
// cross-origin isolated.
class AmberMixer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    // Frames still to play, which the main thread tops up from.
    this.level = 0;
    this.offset = 0;
    this.port.onmessage = ({ data }) => {
      this.queue.push(data);
      this.level += data.length / 2;
    };
  }

  process(_inputs, outputs) {
    const channels = outputs[0];
    const left = channels[0];
    // A context may hand over one channel rather than two. Reading a second
    // that is not there throws, the worklet dies, and with it every report the
    // main thread was pacing itself by -- which is silence from that moment
    // on, and no error anywhere obvious.
    const right = channels[1] || left;
    for (let i = 0; i < left.length; i++) {
      const head = this.queue[0];
      if (!head) {
        left[i] = 0;
        right[i] = 0;
        continue;
      }
      left[i] = head[this.offset];
      right[i] = head[this.offset + 1];
      this.offset += 2;
      this.level--;
      if (this.offset >= head.length) {
        this.queue.shift();
        this.offset = 0;
      }
    }
    // Tell the main thread how much is left so it knows when to send more.
    // Once every block is cheap and keeps the estimate honest.
    this.port.postMessage(this.level);
    return true;
  }
}
registerProcessor("amber-mixer", AmberMixer);
