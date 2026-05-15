// The rate–distortion receipts, and the arithmetic the charts page draws.
//
// The BD-rate here is the same definition `kfmetric` implements natively and
// `bench/metric_oracle.py` checks it against: monotone PCHIP through log-rate
// against quality, integrated analytically over the shared quality interval.
// Three implementations of one definition sounds like two too many, until you
// remember that a chart quietly using a different interpolant from the receipt
// it draws is exactly the kind of disagreement nobody notices.
//
// Nothing here fetches anything the campaign did not write. The page draws the
// numbers in the receipt; it does not re-derive them from something else and
// hope they agree.

export interface RatePoint {
  qp: number;
  bytes: number;
  rate: number;
  quality: number;
  psnrY: number;
  ssimY: number;
  streamSha256: string;
}

export interface Curve {
  clip: string;
  toolset: string;
  width: number;
  height: number;
  frames: number;
  points: RatePoint[];
}

export interface Campaign {
  encoderVersion: string;
  configSha256: string;
  qpLadder: number[];
  curves: Curve[];
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

/// Fritsch–Carlson slopes: what keeps the interpolant from overshooting.
function pchipSlopes(xs: number[], ys: number[]): number[] {
  const count = xs.length;
  const widths: number[] = [];
  const secants: number[] = [];
  for (let index = 0; index < count - 1; index += 1) {
    const width = xs[index + 1]! - xs[index]!;
    widths.push(width);
    secants.push((ys[index + 1]! - ys[index]!) / width);
  }

  const derivatives = new Array<number>(count).fill(0);
  for (let index = 1; index < count - 1; index += 1) {
    const before = secants[index - 1]!;
    const after = secants[index]!;
    if (before * after <= 0) {
      derivatives[index] = 0;
    } else {
      const w1 = 2 * widths[index]! + widths[index - 1]!;
      const w2 = widths[index]! + 2 * widths[index - 1]!;
      derivatives[index] = (w1 + w2) / (w1 / before + w2 / after);
    }
  }
  derivatives[0] = endpointSlope(secants[0]!, secants[1]!, widths[0]!, widths[1]!);
  derivatives[count - 1] = endpointSlope(
    secants[count - 2]!,
    secants[count - 3]!,
    widths[count - 2]!,
    widths[count - 3]!,
  );
  return derivatives;
}

function endpointSlope(near: number, far: number, nearWidth: number, farWidth: number): number {
  if (far === undefined || farWidth === undefined) return near;
  const estimate = ((2 * nearWidth + farWidth) * near - nearWidth * far) / (nearWidth + farWidth);
  if (estimate * near <= 0) return 0;
  if (near * far <= 0 && Math.abs(estimate) > Math.abs(3 * near)) return 3 * near;
  return estimate;
}

/// The exact integral of the cubic Hermite interpolant over `[low, high]`.
function integrate(xs: number[], ys: number[], low: number, high: number): number {
  const derivatives = pchipSlopes(xs, ys);
  let total = 0;
  for (let index = 0; index < xs.length - 1; index += 1) {
    const x0 = xs[index]!;
    const x1 = xs[index + 1]!;
    const start = Math.max(x0, low);
    const end = Math.min(x1, high);
    if (end <= start) continue;
    const width = x1 - x0;
    const y0 = ys[index]!;
    const y1 = ys[index + 1]!;
    const d0 = derivatives[index]!;
    const d1 = derivatives[index + 1]!;
    const at = (x: number) => {
      const t = (x - x0) / width;
      const t2 = t * t;
      const t3 = t2 * t;
      const t4 = t3 * t;
      return (
        y0 * (t4 / 2 - t3 + t) +
        width * d0 * (t4 / 4 - (2 * t3) / 3 + t2 / 2) +
        y1 * (-t4 / 2 + t3) +
        width * d1 * (t4 / 4 - t3 / 3)
      );
    };
    total += width * (at(end) - at(start));
  }
  return total;
}

export type BdRate = { percent: number } | { refused: string };

/// The average bitrate difference of `candidate` against `baseline`.
///
/// Refusals are values rather than exceptions because the page has to render
/// them: "these curves never overlap" is information a reader wants in the
/// table, not a blank cell.
export function bdRate(baseline: RatePoint[], candidate: RatePoint[]): BdRate {
  const prepare = (points: RatePoint[]) => {
    const usable = points
      .filter((point) => Number.isFinite(point.rate) && Number.isFinite(point.quality) && point.rate > 0)
      .map((point) => [point.quality, Math.log(point.rate)] as const)
      .sort((a, b) => a[0] - b[0]);
    const deduplicated: (readonly [number, number])[] = [];
    for (const entry of usable) {
      if (deduplicated.at(-1)?.[0] === entry[0]) continue;
      deduplicated.push(entry);
    }
    return deduplicated;
  };

  const left = prepare(baseline);
  const right = prepare(candidate);
  if (left.length < 4 || right.length < 4) {
    return { refused: "fewer than four distinct points" };
  }
  const low = Math.max(left[0]![0], right[0]![0]);
  const high = Math.min(left.at(-1)![0], right.at(-1)![0]);
  if (!(high > low)) return { refused: "the curves share no quality range" };

  const area = (curve: (readonly [number, number])[]) =>
    integrate(
      curve.map((entry) => entry[0]),
      curve.map((entry) => entry[1]),
      low,
      high,
    );
  const difference = (area(right) - area(left)) / (high - low);
  return { percent: 100 * (Math.exp(difference) - 1) };
}

/// Every clip in the receipt, in the order the campaign ran them.
export function clipsOf(campaign: Campaign): string[] {
  const seen: string[] = [];
  for (const curve of campaign.curves) {
    if (!seen.includes(curve.clip)) seen.push(curve.clip);
  }
  return seen;
}
