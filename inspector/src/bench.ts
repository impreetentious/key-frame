// The rate–distortion receipts, and the reader that turns one into the shapes
// the charts page draws.
//
// There is no arithmetic here. Every published figure — each ablation's BD-rate
// included — is computed by the campaign, written into the receipt, and
// re-derived by `rd_verify` on every run. This file used to carry a third
// implementation of the BD-rate definition, in TypeScript, so the page could
// compute what the receipt already stated. Two implementations exist in order
// to disagree usefully: the Rust one and `bench/metric_oracle.py` check each
// other on committed vectors. A third that nothing compares against is not
// verification, it is a second answer to the same question with no way to
// notice when the two part.
//
// So nothing here fetches anything the campaign did not write, and nothing here
// derives anything the campaign did not measure.

export interface RatePoint {
  qp: number;
  bytes: number;
  rate: number;
  quality: number;
  psnrY: number;
  ssimY: number;
  streamSha256: string;
}

/// A bitrate difference, or the reason the campaign declined to state one.
///
/// Refusals are values rather than exceptions because the page has to render
/// them: "these curves never overlap" is information a reader wants in the
/// table, not a blank cell.
export type BdRate = { percent: number } | { refused: string };

export interface Curve {
  clip: string;
  toolset: string;
  width: number;
  height: number;
  frames: number;
  points: RatePoint[];
  /// What switching this toolset off costs against the full one, as the
  /// campaign measured it. The baseline is not compared against itself, so its
  /// own curve carries nothing.
  bdRate: BdRate | null;
}

export interface AbrTarget {
  targetBps: number;
  achievedBps: number;
  errorPercent: number;
  psnrY: number;
}

export interface RateControlClip {
  clip: string;
  frames: number;
  targets: AbrTarget[];
}

export interface Campaign {
  encoderVersion: string;
  configSha256: string;
  qpLadder: number[];
  curves: Curve[];
  /// Average-bitrate accuracy, kept in its own shape. These are deliberately
  /// not rate-quality points: the quality at an average-bitrate target is an
  /// outcome rather than a setting, so three of them are not a curve and the
  /// page never draws them as one.
  rateControl: RateControlClip[];
}

/// Reads a campaign receipt, refusing anything that is not one.
///
/// A malformed field becomes an exception rather than a `NaN` that reaches a
/// canvas coordinate and silently draws nothing.
export function parseCampaign(data: unknown): Campaign {
  const root = data as Record<string, unknown>;
  if (root?.format !== "key-frame-rd-campaign-v1") {
    throw new Error("this is not a campaign receipt");
  }
  const settings = (root.settings ?? {}) as Record<string, unknown>;
  const rawCurves = root.curves;
  if (!Array.isArray(rawCurves)) throw new Error("the receipt has no curves");

  return {
    encoderVersion: String(root.encoder_version ?? "unknown"),
    configSha256: String(root.config_sha256 ?? ""),
    qpLadder: Array.isArray(settings.qp_ladder) ? settings.qp_ladder.map(Number) : [],
    rateControl: parseRateControl(root.rate_control),
    curves: rawCurves.map((entry) => {
      const curve = entry as Record<string, unknown>;
      const points = curve.points;
      if (!Array.isArray(points)) throw new Error("a curve has no points");
      return {
        clip: String(curve.clip ?? "unnamed"),
        toolset: String(curve.toolset ?? "unnamed"),
        width: Number(curve.width ?? 0),
        height: Number(curve.height ?? 0),
        frames: Number(curve.frames ?? 0),
        bdRate: parseBdRate(curve),
        points: points.map((raw) => {
          const point = raw as Record<string, unknown>;
          return {
            qp: Number(point.qp),
            bytes: Number(point.bytes),
            rate: Number(point.rate),
            quality: Number(point.quality),
            psnrY: Number(point.psnr_y),
            ssimY: Number(point.ssim_y),
            streamSha256: String(point.stream_sha256 ?? ""),
          };
        }),
      };
    }),
  };
}

/// The bitrate difference one curve records, if it records one.
///
/// A figure that is present but not a number is an exception rather than a
/// `NaN` that reaches a table cell and renders as nothing; a figure that is
/// absent is `null`, which the table names rather than leaves blank.
function parseBdRate(curve: Record<string, unknown>): BdRate | null {
  if (curve.bd_rate_percent !== undefined) {
    const percent = Number(curve.bd_rate_percent);
    if (!Number.isFinite(percent)) {
      throw new Error("a curve records a bitrate difference that is not a number");
    }
    return { percent };
  }
  if (curve.bd_rate_refused !== undefined) {
    return { refused: String(curve.bd_rate_refused) };
  }
  return null;
}

function parseRateControl(value: unknown): RateControlClip[] {
  const clips = (value as Record<string, unknown> | undefined)?.clips;
  if (!Array.isArray(clips)) return [];
  return clips.map((entry) => {
    const clip = entry as Record<string, unknown>;
    const targets = Array.isArray(clip.targets) ? clip.targets : [];
    return {
      clip: String(clip.clip ?? "unnamed"),
      frames: Number(clip.frames ?? 0),
      targets: targets.map((raw) => {
        const target = raw as Record<string, unknown>;
        return {
          targetBps: Number(target.target_bps),
          achievedBps: Number(target.achieved_bps),
          errorPercent: Number(target.error_percent),
          psnrY: Number(target.psnr_y),
        };
      }),
    };
  });
}

/// Every clip in the receipt, in the order the campaign ran them.
export function clipsOf(campaign: Campaign): string[] {
  const seen: string[] = [];
  for (const curve of campaign.curves) {
    if (!seen.includes(curve.clip)) seen.push(curve.clip);
  }
  return seen;
}
