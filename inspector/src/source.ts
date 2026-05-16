// Reading a source clip in the browser, and finding the block the codec did
// worst on.
//
// The projection room can always show what the decoder produced. What it cannot
// show, without the original, is how *wrong* it is — decoded video carries no
// record of what it was supposed to look like. So the one question a learner
// most wants answered ("why does this block look bad?") needs a second file,
// and the page refuses to answer it without one rather than estimating an error
// it cannot measure.
//
// That refusal is the whole design here. A heuristic standing in for a source
// comparison would produce a confident-looking answer that is sometimes simply
// wrong, and a page built to be trusted cannot afford one of those.

import type { DecodedFrame } from "./decoder";
import { type CodingBlock, type FrameReport, allBlocks, modeledBits } from "./probe";

export interface SourceClip {
  width: number;
  height: number;
  frameCount: number;
  /// Luma planes only, tightly packed, in display order. Chroma is parsed for
  /// its length and discarded: every metric on this page is luma.
  luma: Uint8Array[];
}

export class SourceError extends Error {}

/// Parses strict 8-bit 4:2:0 YUV4MPEG2, the one format the codec accepts.
///
/// Deliberately narrow. A permissive reader would accept a 4:2:2 or 10-bit clip
/// and silently compare the wrong bytes, which is worse than refusing: the
/// resulting error map would look plausible and mean nothing.
export function parseY4m(bytes: Uint8Array): SourceClip {
  const text = new TextDecoder("ascii", { fatal: false });
  const headerEnd = bytes.indexOf(0x0a);
  if (headerEnd < 0) throw new SourceError("this file has no YUV4MPEG2 header");

  const header = text.decode(bytes.subarray(0, headerEnd));
  const tags = header.split(" ");
  if (tags[0] !== "YUV4MPEG2") throw new SourceError("this file is not a Y4M clip");

  let width = 0;
  let height = 0;
  let chroma = "420jpeg";
  for (const tag of tags.slice(1)) {
    const kind = tag[0];
    const value = tag.slice(1);
    if (kind === "W") width = Number.parseInt(value, 10);
    else if (kind === "H") height = Number.parseInt(value, 10);
    else if (kind === "C") chroma = value;
    else if (kind === "I" && value !== "p") {
      throw new SourceError("only progressive clips are supported");
    }
  }
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
    throw new SourceError("the header does not give usable dimensions");
  }
  // The codec's own reader treats an absent C tag as C420jpeg, so this does too.
  if (!["420jpeg", "420", "420mpeg2", "420paldv"].includes(chroma)) {
    throw new SourceError(`chroma ${chroma} is not 8-bit 4:2:0`);
  }

  const lumaSize = width * height;
  const chromaSize = Math.ceil(width / 2) * Math.ceil(height / 2);
  const frameSize = lumaSize + 2 * chromaSize;

  const luma: Uint8Array[] = [];
  let at = headerEnd + 1;
  while (at < bytes.length) {
    const markEnd = bytes.indexOf(0x0a, at);
    if (markEnd < 0) throw new SourceError("a frame header is unterminated");
    const mark = text.decode(bytes.subarray(at, markEnd));
    if (!mark.startsWith("FRAME")) {
      throw new SourceError(`expected a FRAME marker, found ${JSON.stringify(mark.slice(0, 16))}`);
    }
    const start = markEnd + 1;
    if (start + frameSize > bytes.length) throw new SourceError("the last frame is truncated");
    luma.push(bytes.subarray(start, start + lumaSize));
    at = start + frameSize;
  }
  if (luma.length === 0) throw new SourceError("the clip has no frames");

  return { width, height, frameCount: luma.length, luma };
}

export interface WorstBlock {
  block: CodingBlock;
  /// Mean squared luma error over the block, against the source.
  meanSquaredError: number;
  /// Modeled bits the encoder predicted for it.
  bits: number;
}

/// The block that went worst: most error for the fewest bits.
///
/// Ranked by error per modeled bit rather than by error alone. Ranking by error
/// alone always points at whichever block is most detailed, which is not a
/// finding — hard blocks are hard. The interesting block is the one the encoder
/// *decided* to spend little on and then got badly wrong, because that is a
/// decision a reader can go and look at.
///
/// Blocks with no error are skipped outright rather than scoring zero, so a
/// clip the codec happened to code perfectly reports nothing instead of an
/// arbitrary pick.
export function worstBlock(
  report: FrameReport,
  decoded: DecodedFrame,
  source: Uint8Array,
): WorstBlock | null {
  const width = decoded.width;
  let worst: WorstBlock | null = null;
  let worstRatio = 0;

  for (const block of allBlocks(report)) {
    let squaredError = 0;
    let samples = 0;
    for (let row = 0; row < block.size; row += 1) {
      const y = block.y + row;
      if (y >= decoded.height) break;
      for (let column = 0; column < block.size; column += 1) {
        const x = block.x + column;
        if (x >= width) break;
        const index = y * width + x;
        const difference = (source[index] ?? 0) - (decoded.y[index] ?? 0);
        squaredError += difference * difference;
        samples += 1;
      }
    }
    if (samples === 0 || squaredError === 0) continue;

    const meanSquaredError = squaredError / samples;
    // A floor of one bit, so a block the model priced at almost nothing cannot
    // divide its way to the top of the ranking on rounding alone.
    const bits = Math.max(1, modeledBits(block.modeledEntropyQ16));
    const ratio = meanSquaredError / bits;
    if (ratio > worstRatio) {
      worstRatio = ratio;
      worst = { block, meanSquaredError, bits };
    }
  }
  return worst;
}
