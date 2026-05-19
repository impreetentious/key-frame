// The browser half of the raw WebAssembly boundary.
//
// The module imports nothing, so there is no glue file to keep in step and no
// generator version to pin: `WebAssembly.instantiate` with an empty import
// object is the whole loading story. Everything below is the other half of the
// contract written at the top of `crates/kf-wasm/src/abi.rs`.
//
// One rule governs every view of the module's memory: take it *after* the call
// that decided where the bytes are, never before. Any allocation inside the
// module may have grown its memory, and a `Uint8Array` over the old buffer is
// detached the moment that happens. Every read goes through `readBytes`, which
// copies; the single write is the one in `open`, taken after `kf_alloc` has
// returned the offset it is writing to.

const ABI_VERSION = 1;

/// Status codes, as numbered by the module. The text is for people; code that
/// branches on failure branches on the number.
const STATUS_TEXT: Record<number, string> = {
  0: "ok",
  1: "no stream is open",
  2: "the stream is not a usable version-one stream",
  3: "that frame is not in this stream",
  4: "the frame did not decode",
  5: "the frame did not probe",
};

export class DecoderError extends Error {
  readonly status: number;

  constructor(status: number, detail: string) {
    const summary = STATUS_TEXT[status] ?? `status ${status}`;
    super(detail ? `${summary}: ${detail}` : summary);
    this.name = "DecoderError";
    this.status = status;
  }
}

interface Exports {
  memory: WebAssembly.Memory;
  kf_abi_version(): number;
  kf_alloc(len: number): number;
  kf_free(offset: number, len: number): void;
  kf_open(offset: number, len: number): number;
  kf_decode_frame(index: number): number;
  kf_decode_all(): number;
  kf_probe_frame(index: number): number;
  kf_width(): number;
  kf_height(): number;
  kf_fps_num(): number;
  kf_fps_den(): number;
  kf_frame_count(): number;
  kf_keyframe_count(): number;
  kf_keyframe_at(position: number): number;
  kf_last_entry_keyframe(): number;
  kf_last_entry_cost(): number;
  kf_output_ptr(): number;
  kf_output_len(): number;
  kf_message_ptr(): number;
  kf_message_len(): number;
}

export interface StreamInfo {
  width: number;
  height: number;
  fpsNum: number;
  fpsDen: number;
  frameCount: number;
  keyframes: number[];
}

/// One decoded frame, still in the codec's own planar form.
///
/// The planes are handed over as they came out of the decoder rather than
/// converted on the way: the overlays measure in luma samples, and converting
/// early would mean converting back.
export interface DecodedFrame {
  index: number;
  width: number;
  height: number;
  y: Uint8Array;
  cb: Uint8Array;
  cr: Uint8Array;
  /// The keyframe this frame's decode restarted from, and what that cost. A
  /// scrub bar can show why a jump backwards was slower than a step forwards.
  entryKeyframe: number;
  entryCost: number;
}

export class Decoder {
  private readonly exports: Exports;
  private info: StreamInfo | null = null;

  private constructor(exports: Exports) {
    this.exports = exports;
  }

  static async load(moduleUrl: string): Promise<Decoder> {
    const response = await fetch(moduleUrl);
    if (!response.ok) {
      throw new Error(`the decoder module did not load (${response.status})`);
    }
    const { instance } = await WebAssembly.instantiateStreaming(response, {}).catch(
      async () => {
        // Servers that mislabel the media type break streaming instantiation.
        // Falling back keeps a static host from being a reason the page fails.
        const bytes = await (await fetch(moduleUrl)).arrayBuffer();
        return WebAssembly.instantiate(bytes, {});
      },
    );
    const exports = instance.exports as unknown as Exports;
    const version = exports.kf_abi_version();
    if (version !== ABI_VERSION) {
      throw new Error(
        `the decoder module speaks boundary version ${version}, this page speaks ${ABI_VERSION}`,
      );
    }
    return new Decoder(exports);
  }

  /// Hands a complete stream to the module. Nothing is decoded yet: this
  /// validates the header and the packet sequence and reads the shape.
  open(stream: Uint8Array): StreamInfo {
    const offset = this.exports.kf_alloc(stream.length);
    if (offset === 0 && stream.length > 0) {
      throw new Error("the decoder could not allocate room for the stream");
    }
    new Uint8Array(this.exports.memory.buffer, offset, stream.length).set(stream);
    const status = this.exports.kf_open(offset, stream.length);
    this.exports.kf_free(offset, stream.length);
    if (status !== 0) {
      this.info = null;
      throw new DecoderError(status, this.message());
    }
    const keyframes: number[] = [];
    for (let position = 0; position < this.exports.kf_keyframe_count(); position += 1) {
      keyframes.push(this.exports.kf_keyframe_at(position));
    }
    this.info = {
      width: this.exports.kf_width(),
      height: this.exports.kf_height(),
      fpsNum: this.exports.kf_fps_num(),
      fpsDen: this.exports.kf_fps_den(),
      frameCount: this.exports.kf_frame_count(),
      keyframes,
    };
    return this.info;
  }

  get stream(): StreamInfo | null {
    return this.info;
  }

  /// Decodes one frame by random access.
  decodeFrame(index: number): DecodedFrame {
    const info = this.requireStream();
    const status = this.exports.kf_decode_frame(index);
    if (status !== 0) throw new DecoderError(status, this.message());
    const planes = this.readBytes(
      this.exports.kf_output_ptr(),
      this.exports.kf_output_len(),
    );
    const lumaSize = info.width * info.height;
    const chromaWidth = Math.ceil(info.width / 2);
    const chromaHeight = Math.ceil(info.height / 2);
    const chromaSize = chromaWidth * chromaHeight;
    if (planes.length !== lumaSize + 2 * chromaSize) {
      throw new Error(
        `frame ${index} came back as ${planes.length} bytes, expected ${lumaSize + 2 * chromaSize}`,
      );
    }
    return {
      index,
      width: info.width,
      height: info.height,
      y: planes.subarray(0, lumaSize),
      cb: planes.subarray(lumaSize, lumaSize + chromaSize),
      cr: planes.subarray(lumaSize + chromaSize),
      entryKeyframe: this.exports.kf_last_entry_keyframe(),
      entryCost: this.exports.kf_last_entry_cost(),
    };
  }

  /// The syntax report for one frame, parsed from the module's JSON.
  probeFrame(index: number): unknown {
    this.requireStream();
    const status = this.exports.kf_probe_frame(index);
    if (status !== 0) throw new DecoderError(status, this.message());
    const bytes = this.readBytes(
      this.exports.kf_output_ptr(),
      this.exports.kf_output_len(),
    );
    return JSON.parse(new TextDecoder().decode(bytes));
  }

  private requireStream(): StreamInfo {
    if (!this.info) throw new DecoderError(1, "");
    return this.info;
  }

  /// The only place that reads the module's memory buffer, and it always
  /// copies: a view handed to a caller would be silently detached by the next
  /// allocation. The one write lives in `open`, where the offset comes from the
  /// `kf_alloc` immediately above it.
  private readBytes(offset: number, length: number): Uint8Array {
    return new Uint8Array(this.exports.memory.buffer, offset, length).slice();
  }

  private message(): string {
    const length = this.exports.kf_message_len();
    if (length === 0) return "";
    return new TextDecoder().decode(
      this.readBytes(this.exports.kf_message_ptr(), length),
    );
  }
}
