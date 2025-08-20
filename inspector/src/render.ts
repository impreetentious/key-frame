// Turning decoded planes into pixels, and drawing the six overlays over them.
//
// Everything here is hand-rolled 2D canvas work. There is no charting library
// and no video element: the whole point of the page is that what you see came
// out of the decoder in this tab, and a library between the two would be one
// more thing to have to trust.

import type { DecodedFrame } from "./decoder";
import { type CodingBlock, type FrameReport, allBlocks, modeledBits } from "./probe";

export const OVERLAY_NAMES = [
  "partition",
  "intra",
  "motion",
  "entropy",
  "qp",
  "residual",
] as const;

export type OverlayName = (typeof OVERLAY_NAMES)[number];

export type Overlays = Record<OverlayName, boolean>;

export const OVERLAY_LABELS: Record<OverlayName, string> = {
  partition: "Partition grid",
  intra: "Intra glyphs",
  motion: "Motion vectors",
  entropy: "Entropy heatmap",
  qp: "Frame QP tint",
  residual: "Residual energy",
};

export const NO_OVERLAYS: Overlays = {
  partition: false,
  intra: false,
  motion: false,
  entropy: false,
  qp: false,
  residual: false,
};

/// Which quantity the heatmap colours.
///
/// They are different questions and the page never blurs them: modeled entropy
/// is what the encoder's model predicted, emission-time bytes is when the coder
/// happened to flush. Neither is "the bits this block owns".
export type HeatmapBasis = "modeled" | "emitted";

/// Full-range BT.601, matching the `C420jpeg` siting the format accepts.
///
/// Chroma is upsampled by nearest neighbour rather than interpolated: the point
/// is to show what the decoder produced, and a smoothing filter here would
/// quietly improve the picture the codec is being judged on.
export function frameToImageData(frame: DecodedFrame): ImageData {
  const { width, height, y: luma, cb, cr } = frame;
  const chromaWidth = Math.ceil(width / 2);
  const image = new ImageData(width, height);
  const pixels = image.data;
  for (let row = 0; row < height; row += 1) {
    const chromaRow = row >> 1;
    for (let column = 0; column < width; column += 1) {
      const chromaIndex = chromaRow * chromaWidth + (column >> 1);
      const sampleY = luma[row * width + column] ?? 0;
      const sampleU = (cb[chromaIndex] ?? 128) - 128;
      const sampleV = (cr[chromaIndex] ?? 128) - 128;
      const offset = (row * width + column) * 4;
      pixels[offset] = clampByte(sampleY + 1.402 * sampleV);
      pixels[offset + 1] = clampByte(sampleY - 0.344136 * sampleU - 0.714136 * sampleV);
      pixels[offset + 2] = clampByte(sampleY + 1.772 * sampleU);
      pixels[offset + 3] = 255;
    }
  }
  return image;
}

function clampByte(value: number): number {
  return value < 0 ? 0 : value > 255 ? 255 : Math.round(value);
}

interface OverlayContext {
  context: CanvasRenderingContext2D;
  report: FrameReport;
  scale: number;
  basis: HeatmapBasis;
  selected: CodingBlock | null;
}

export function drawOverlays(
  canvas: HTMLCanvasElement,
  report: FrameReport,
  overlays: Overlays,
  basis: HeatmapBasis,
  selected: CodingBlock | null,
  scale: number,
): void {
  const context = canvas.getContext("2d");
  if (!context) return;
  context.clearRect(0, 0, canvas.width, canvas.height);
  const state: OverlayContext = { context, report, scale, basis, selected };

  // Order matters: tints and heatmaps fill, so they go under the strokes and
  // glyphs that have to stay readable on top of them.
  if (overlays.qp) drawQpTint(state);
  if (overlays.entropy) drawEntropyHeatmap(state);
  if (overlays.residual) drawResidualEnergy(state);
  if (overlays.partition) drawPartitionGrid(state);
  if (overlays.intra) drawIntraGlyphs(state);
  if (overlays.motion) drawMotionVectors(state);
  if (selected) drawSelection(state, selected);
}

function drawQpTint({ context, report }: OverlayContext): void {
  // One QP for the whole frame in version one, so the tint is a frame-level
  // wash rather than a per-block one. Warmer means coarser.
  const warmth = Math.min(1, report.qp / 63);
  context.fillStyle = `rgba(${Math.round(120 + 135 * warmth)}, ${Math.round(
    120 - 60 * warmth,
  )}, 40, 0.14)`;
  context.fillRect(0, 0, context.canvas.width, context.canvas.height);

  // The label is a fixed size in screen pixels, not a multiple of the zoom.
  // The overlay canvas is sized in display pixels, so scaling the text with the
  // picture would let a small clip at high zoom cover its own frame with a
  // caption about that frame.
  const label = `QP ${report.qp}`;
  context.font = "12px ui-monospace, monospace";
  const width = context.measureText(label).width;
  context.fillStyle = "rgba(10, 12, 16, 0.6)";
  context.fillRect(6, 6, width + 10, 18);
  context.fillStyle = "rgba(255, 240, 214, 0.94)";
  context.fillText(label, 11, 19);
}

function heatValue(block: CodingBlock, basis: HeatmapBasis): number {
  return basis === "modeled" ? modeledBits(block.modeledEntropyQ16) : block.emittedPayloadBytes;
}

function drawEntropyHeatmap(state: OverlayContext): void {
  const { context, report, scale, basis } = state;
  const blocks = allBlocks(report);
  const peak = blocks.reduce((highest, block) => Math.max(highest, heatValue(block, basis)), 0);
  if (peak <= 0) return;
  for (const block of blocks) {
    const share = heatValue(block, basis) / peak;
    // A perceptual-ish ramp: cool and transparent for cheap blocks, hot and
    // opaque for expensive ones, so a glance finds where the bits went.
    context.fillStyle = `rgba(${Math.round(40 + 215 * share)}, ${Math.round(
      70 + 90 * (1 - share),
    )}, ${Math.round(180 - 150 * share)}, ${0.12 + 0.5 * share})`;
    context.fillRect(block.x * scale, block.y * scale, block.size * scale, block.size * scale);
  }
}

function drawResidualEnergy({ context, report, scale }: OverlayContext): void {
  const blocks = allBlocks(report);
  const peak = blocks.reduce((highest, block) => Math.max(highest, block.dcEnergy), 0);
  if (peak <= 0) return;
  for (const block of blocks) {
    const share = block.dcEnergy / peak;
    if (share === 0) continue;
    context.fillStyle = `rgba(96, 232, 168, ${0.1 + 0.45 * share})`;
    context.fillRect(block.x * scale, block.y * scale, block.size * scale, block.size * scale);
  }
}

function drawPartitionGrid({ context, report, scale }: OverlayContext): void {
  context.lineWidth = 1;
  for (const superblock of report.superblocks) {
    for (const block of superblock.blocks) {
      context.strokeStyle = "rgba(255, 255, 255, 0.34)";
      context.strokeRect(
        block.x * scale + 0.5,
        block.y * scale + 0.5,
        block.size * scale - 1,
        block.size * scale - 1,
      );
    }
    // The superblock boundary is the unit the format actually iterates, so it
    // is drawn brighter than the quadtree cuts inside it.
    context.strokeStyle = "rgba(255, 214, 122, 0.75)";
    context.strokeRect(
      superblock.x * scale + 0.5,
      superblock.y * scale + 0.5,
      superblock.size * scale - 1,
      superblock.size * scale - 1,
    );
  }
}

/// One glyph per intra mode, drawn as the direction the mode predicts from.
/// Directional modes get a line at their angle; DC and planar get their own
/// marks, because neither has a direction to draw.
const INTRA_ANGLES: Record<string, number> = {
  horizontal: 0,
  vertical: 90,
  d45: 45,
  d135: 135,
  d117: 117,
  d153: 153,
};

function drawIntraGlyphs({ context, report, scale }: OverlayContext): void {
  context.lineWidth = 1.5;
  context.strokeStyle = "rgba(140, 210, 255, 0.9)";
  for (const block of allBlocks(report)) {
    if (block.prediction.kind !== "intra") continue;
    const mode = block.prediction.mode ?? "dc";
    const centreX = (block.x + block.size / 2) * scale;
    const centreY = (block.y + block.size / 2) * scale;
    const reach = (block.size * scale) / 3;
    const angle = INTRA_ANGLES[mode];
    context.beginPath();
    if (angle === undefined) {
      if (mode === "planar") {
        // Two crossed strokes: a plane is fitted from both edges at once.
        context.moveTo(centreX - reach, centreY - reach);
        context.lineTo(centreX + reach, centreY + reach);
        context.moveTo(centreX - reach, centreY + reach);
        context.lineTo(centreX + reach, centreY - reach);
      } else {
        // DC: a flat average, drawn flat.
        context.arc(centreX, centreY, Math.max(2, reach / 2), 0, Math.PI * 2);
      }
    } else {
      const radians = (angle * Math.PI) / 180;
      context.moveTo(centreX - Math.cos(radians) * reach, centreY + Math.sin(radians) * reach);
      context.lineTo(centreX + Math.cos(radians) * reach, centreY - Math.sin(radians) * reach);
    }
    context.stroke();
  }
}

function drawMotionVectors({ context, report, scale }: OverlayContext): void {
  context.lineWidth = 1.5;
  for (const block of allBlocks(report)) {
    const { kind, reference, mvQ4 } = block.prediction;
    if (kind === "intra") continue;
    const centreX = (block.x + block.size / 2) * scale;
    const centreY = (block.y + block.size / 2) * scale;
    // LAST and GOLDEN are told apart by colour, because a vector's length says
    // nothing about which picture it points into.
    context.strokeStyle =
      reference === "golden" ? "rgba(255, 176, 88, 0.95)" : "rgba(120, 255, 190, 0.95)";
    if (!mvQ4) {
      // A skipped block codes no difference at all: it inherits its predictor.
      // Drawing an arrow of zero length would claim it chose to stay still.
      context.beginPath();
      context.arc(centreX, centreY, 2.5, 0, Math.PI * 2);
      context.stroke();
      continue;
    }
    // Quarter-pel units, and the difference is drawn from the block centre.
    const endX = centreX + (mvQ4[0] / 4) * scale;
    const endY = centreY + (mvQ4[1] / 4) * scale;
    context.beginPath();
    context.moveTo(centreX, centreY);
    context.lineTo(endX, endY);
    context.stroke();
    const head = Math.atan2(endY - centreY, endX - centreX);
    context.beginPath();
    context.moveTo(endX, endY);
    context.lineTo(endX - 5 * Math.cos(head - Math.PI / 7), endY - 5 * Math.sin(head - Math.PI / 7));
    context.moveTo(endX, endY);
    context.lineTo(endX - 5 * Math.cos(head + Math.PI / 7), endY - 5 * Math.sin(head + Math.PI / 7));
    context.stroke();
  }
}

function drawSelection({ context, scale }: OverlayContext, block: CodingBlock): void {
  context.lineWidth = 2;
  context.strokeStyle = "rgba(255, 255, 255, 0.95)";
  context.strokeRect(
    block.x * scale + 1,
    block.y * scale + 1,
    block.size * scale - 2,
    block.size * scale - 2,
  );
}
