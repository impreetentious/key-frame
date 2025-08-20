import { type CodingBlock, type FrameReport, modeledBits } from "./probe";

/// The syntax of one coding block, with both accounting figures side by side
/// and neither of them called "the bits this block cost".
export function BlockPanel({
  report,
  block,
}: {
  report: FrameReport | null;
  block: CodingBlock | null;
}) {
  if (!report) return null;
  if (!block) {
    return (
      <div className="card">
        <h2>Block</h2>
        <p className="hint">Click the picture to select a coding block.</p>
      </div>
    );
  }

  const superblock = report.superblocks.find((candidate) =>
    candidate.blocks.some((member) => member.x === block.x && member.y === block.y),
  );

  return (
    <div className="card">
      <h2>Block</h2>
      <dl>
        <dt>Position</dt>
        <dd>
          ({block.x}, {block.y}) at {block.size}×{block.size}
        </dd>
        <dt>Prediction</dt>
        <dd>{describePrediction(block)}</dd>
        <dt>QP</dt>
        <dd>{block.qp}</dd>
        <dt>Modeled entropy</dt>
        <dd title="What the encoder's cost model predicted for this block, in bits. Not a measurement.">
          {modeledBits(block.modeledEntropyQ16).toFixed(2)} bits
        </dd>
        <dt>Emission-time bytes</dt>
        <dd title="Bytes the canonical replay flushed while this block was being coded. Delayed carry means this is timing, not ownership.">
          {block.emittedPayloadBytes}
        </dd>
        <dt>DC energy</dt>
        <dd>{block.dcEnergy}</dd>
      </dl>

      {superblock ? (
        <>
          <h3>Its superblock&apos;s structure</h3>
          <dl>
            <dt>Position</dt>
            <dd>
              ({superblock.x}, {superblock.y})
            </dd>
            <dt>Structure modeled entropy</dt>
            <dd title="The cost the model assigned to the partition tree itself, kept separate from the blocks inside it.">
              {modeledBits(superblock.structureModeledEntropyQ16).toFixed(2)} bits
            </dd>
            <dt>Structure emission-time bytes</dt>
            <dd>{superblock.structureEmittedPayloadBytes}</dd>
          </dl>
        </>
      ) : null}

      <p className="disclaimer">
        The two accounting figures answer different questions and neither answers &ldquo;how many
        bits is this block in the file&rdquo;. That question has no answer in an adaptive
        arithmetic coder.
      </p>
    </div>
  );
}

function describePrediction(block: CodingBlock): string {
  const { kind, mode, reference, mvQ4 } = block.prediction;
  if (kind === "intra") return `intra, ${mode ?? "unknown"}`;
  if (kind === "skip") return `skip from ${reference ?? "unknown"}`;
  if (!mvQ4) return `inter from ${reference ?? "unknown"}`;
  const [x, y] = mvQ4;
  return `inter from ${reference ?? "unknown"}, difference (${x / 4}, ${y / 4}) pixels`;
}
