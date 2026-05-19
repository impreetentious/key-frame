// The shape of the probe's report, and the one place that turns its untyped
// JSON into something the rest of the page can trust.
//
// The report crosses the WebAssembly boundary as text, so nothing about it is
// guaranteed by the type system on this side. `parseReport` checks every field
// it reads and refuses the whole document rather than letting a missing number
// become `undefined` three components later, where it would be drawn as a
// blank overlay and read as "this block cost nothing".

export type PredictionKind = "intra" | "inter" | "skip";

export interface Prediction {
  kind: PredictionKind;
  mode?: string;
  reference?: string;
  mvQ4?: [number, number];
}

export interface CodingBlock {
  x: number;
  y: number;
  size: number;
  prediction: Prediction;
  qp: number;
  /// Q16.16 fixed point. This is what the *model* said the block should cost,
  /// not what it did cost. See `modeledBits`.
  modeledEntropyQ16: number;
  /// Bytes the canonical replay emitted while this block was being coded.
  /// Delayed carry means that is a timing bucket, never symbol ownership.
  emittedPayloadBytes: number;
  dcEnergy: number;
}

export interface Superblock {
  x: number;
  y: number;
  size: number;
  structureModeledEntropyQ16: number;
  structureEmittedPayloadBytes: number;
  blocks: CodingBlock[];
}

export interface FrameReport {
  frameIndex: number;
  key: boolean;
  goldenRefresh: boolean;
  show: boolean;
  qp: number;
  inputPayloadLen: number;
  canonicalReplayPayloadLen: number;
  canonicalPayloadMatch: boolean;
  firstMismatchOffset: number | null;
  frameFlushBytes: number;
  superblocks: Superblock[];
}

export class ReportError extends Error {}

function asRecord(value: unknown, where: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ReportError(`${where} is not an object`);
  }
  return value as Record<string, unknown>;
}

function asNumber(record: Record<string, unknown>, key: string, where: string): number {
  const value = record[key];
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new ReportError(`${where}.${key} is not a number`);
  }
  return value;
}

function asBoolean(record: Record<string, unknown>, key: string, where: string): boolean {
  const value = record[key];
  if (typeof value !== "boolean") throw new ReportError(`${where}.${key} is not a boolean`);
  return value;
}

function asArray(record: Record<string, unknown>, key: string, where: string): unknown[] {
  const value = record[key];
  if (!Array.isArray(value)) throw new ReportError(`${where}.${key} is not an array`);
  return value;
}

function asPair(record: Record<string, unknown>, key: string, where: string): [number, number] {
  const value = record[key];
  if (!Array.isArray(value) || value.length !== 2) {
    throw new ReportError(`${where}.${key} is not a pair`);
  }
  const [first, second] = value;
  // Finite, not merely `number`. `typeof NaN` is `"number"`, and this is the
  // one parser standing between the module's text and a canvas coordinate:
  // a pair that reached `blockAt` as `[NaN, NaN]` would match no sample and
  // draw no glyph, which reads on the page as a block that cost nothing rather
  // than as a report this page could not trust. Every scalar field is already
  // held to this; a position and a motion vector were not.
  if (!Number.isFinite(first) || !Number.isFinite(second)) {
    throw new ReportError(`${where}.${key} is not a pair of numbers`);
  }
  return [first as number, second as number];
}

function parsePrediction(value: unknown, where: string): Prediction {
  const record = asRecord(value, where);
  const kind = record["kind"];
  if (kind !== "intra" && kind !== "inter" && kind !== "skip") {
    throw new ReportError(`${where}.kind is not a prediction kind`);
  }
  const prediction: Prediction = { kind };
  if (typeof record["mode"] === "string") prediction.mode = record["mode"];
  if (typeof record["reference"] === "string") prediction.reference = record["reference"];
  if (record["mv_q4"] !== undefined) prediction.mvQ4 = asPair(record, "mv_q4", where);
  return prediction;
}

function parseBlock(value: unknown, where: string): CodingBlock {
  const record = asRecord(value, where);
  const [x, y] = asPair(record, "pos", where);
  return {
    x,
    y,
    size: asNumber(record, "size", where),
    prediction: parsePrediction(record["prediction"], `${where}.prediction`),
    qp: asNumber(record, "qp", where),
    modeledEntropyQ16: asNumber(record, "modeled_entropy_q16", where),
    emittedPayloadBytes: asNumber(record, "emitted_payload_bytes", where),
    dcEnergy: asNumber(record, "dc_energy", where),
  };
}

function parseSuperblock(value: unknown, where: string): Superblock {
  const record = asRecord(value, where);
  const [x, y] = asPair(record, "pos", where);
  return {
    x,
    y,
    size: asNumber(record, "size", where),
    structureModeledEntropyQ16: asNumber(record, "structure_modeled_entropy_q16", where),
    structureEmittedPayloadBytes: asNumber(record, "structure_emitted_payload_bytes", where),
    blocks: asArray(record, "cbs", where).map((block, index) =>
      parseBlock(block, `${where}.cbs[${index}]`),
    ),
  };
}

export function parseReport(value: unknown): FrameReport {
  const root = asRecord(value, "report");
  const version = asNumber(root, "probe_version", "report");
  if (version !== 1) {
    throw new ReportError(`the report is probe version ${version}, this page reads version 1`);
  }
  const frame = asRecord(root["frame"], "report.frame");
  const flags = asRecord(frame["flags"], "report.frame.flags");
  const mismatch = frame["first_mismatch_offset"];
  // Finite for the same reason a position is: a mismatch offset is the byte a
  // reader is told to look at, and `NaN` there names no byte.
  if (mismatch !== null && !Number.isFinite(mismatch)) {
    throw new ReportError("report.frame.first_mismatch_offset is neither null nor a number");
  }
  return {
    frameIndex: asNumber(frame, "frame_index", "report.frame"),
    key: asBoolean(flags, "key", "report.frame.flags"),
    goldenRefresh: asBoolean(flags, "golden_refresh", "report.frame.flags"),
    show: asBoolean(flags, "show", "report.frame.flags"),
    qp: asNumber(frame, "qp", "report.frame"),
    inputPayloadLen: asNumber(frame, "input_payload_len", "report.frame"),
    canonicalReplayPayloadLen: asNumber(frame, "canonical_replay_payload_len", "report.frame"),
    canonicalPayloadMatch: asBoolean(frame, "canonical_payload_match", "report.frame"),
    firstMismatchOffset: mismatch as number | null,
    frameFlushBytes: asNumber(frame, "frame_flush_bytes", "report.frame"),
    superblocks: asArray(frame, "superblocks", "report.frame").map((superblock, index) =>
      parseSuperblock(superblock, `report.frame.superblocks[${index}]`),
    ),
  };
}

/// The modeled cost in bits. Q16.16, so sixteen fractional bits.
///
/// This is the encoder's *model* of what the block should have cost, which is
/// what the rate-distortion search actually optimized. It is not a measurement
/// of the bits this block occupies in the file — an adaptive arithmetic coder
/// does not give any block a private set of bits to occupy.
export function modeledBits(modeledEntropyQ16: number): number {
  return modeledEntropyQ16 / 65536;
}

/// Every coding block of a frame, flattened, in decode order.
export function allBlocks(report: FrameReport): CodingBlock[] {
  return report.superblocks.flatMap((superblock) => superblock.blocks);
}

/// The block containing a luma sample, or null outside every block.
///
/// Blocks tile the padded frame without overlapping, so the first hit is the
/// only hit.
export function blockAt(report: FrameReport, x: number, y: number): CodingBlock | null {
  for (const superblock of report.superblocks) {
    for (const block of superblock.blocks) {
      if (
        x >= block.x &&
        x < block.x + block.size &&
        y >= block.y &&
        y < block.y + block.size
      ) {
        return block;
      }
    }
  }
  return null;
}
