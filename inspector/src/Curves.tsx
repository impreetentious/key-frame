import { useEffect, useMemo, useRef, useState } from "react";

import {
  type Campaign,
  type Curve,
  type RateControlClip,
  type RatePoint,
  clipsOf,
  parseCampaign,
} from "./bench";

// Written by `scripts/build-inspector.sh` from `bench/results/`. The page never
// computes a rate–distortion point; it draws the ones the campaign measured and
// the verifier re-derives on every run.
const CAMPAIGN_URL = "./rd-campaign.json";

/// One colour per toolset, stable across every chart and the table.
///
/// The full toolset is the accent colour and everything else is a muted hue, so
/// a reader can tell the baseline from the ablations without reading a legend.
const TOOLSET_COLOURS: Record<string, string> = {
  full: "#ffd67a",
  "no-golden": "#7cc4e0",
  "no-skip": "#a8e07c",
  "no-inter": "#ff9d7a",
  "no-subpel": "#c4a8e0",
  "no-split": "#e07cb4",
};

const TOOLSET_NOTES: Record<string, string> = {
  full: "Every tool on. The shipping encoder, and the baseline for every figure below.",
  "no-golden":
    "Only the LAST reference is offered to the search. The golden slot is still maintained exactly as the format requires.",
  "no-skip": "No block may be coded as its predictor with no residual.",
  "no-inter":
    "P frames stay P frames, but every block in them is coded with an intra mode.",
  "no-subpel":
    "Motion search stops at integer positions; the quarter-pixel interpolator is never asked for.",
  "no-split": "Every superblock is a single 64×64 leaf.",
};

type Metric = "psnrY" | "ssimY";

const METRIC_LABELS: Record<Metric, string> = {
  psnrY: "PSNR-Y (dB)",
  ssimY: "SSIM-Y",
};

export function Curves() {
  const [campaign, setCampaign] = useState<Campaign | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [metric, setMetric] = useState<Metric>("psnrY");

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const response = await fetch(CAMPAIGN_URL);
        if (!response.ok)
          throw new Error(`the receipts did not load (${response.status})`);
        const parsed = parseCampaign(await response.json());
        if (!cancelled) setCampaign(parsed);
      } catch (caught) {
        if (!cancelled)
          setError(caught instanceof Error ? caught.message : String(caught));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <section className="catalogue">
        <p className="error" role="alert">
          {error}
        </p>
      </section>
    );
  }
  if (!campaign)
    return <section className="catalogue">Loading the receipts…</section>;

  const clips = clipsOf(campaign);

  return (
    <section className="catalogue wide">
      <p className="lede">
        What each encoder tool is worth, measured by turning it off. Every curve
        below was produced by this repository's encoder, decoded by both of its
        decoders, and required to agree sample for sample before a single number
        was recorded.
      </p>

      <div className="caveat">
        <strong>Orientation, not a race.</strong> These are comparisons of this
        codec against itself. Nothing here is measured against a mature standard
        encoder, because a deliberately readable scalar codec losing to decades
        of tuning is not a finding, and framing it as one would be the least
        honest thing on the page.
      </div>

      <fieldset className="basis">
        <legend>Quality metric</legend>
        {(Object.keys(METRIC_LABELS) as Metric[]).map((name) => (
          <label key={name}>
            <input
              type="radio"
              name="rd-metric"
              checked={metric === name}
              onChange={() => setMetric(name)}
            />
            {METRIC_LABELS[name]}
          </label>
        ))}
      </fieldset>

      {clips.map((clip) => (
        <ClipSection
          key={clip}
          campaign={campaign}
          clip={clip}
          metric={metric}
        />
      ))}

      <RateControlTable clips={campaign.rateControl} />

      <p className="disclaimer">
        Encoder <code>v{campaign.encoderVersion}</code>, configuration{" "}
        <code>{campaign.configSha256.slice(0, 16)}</code>, QP ladder{" "}
        {campaign.qpLadder.join(", ")}. Every point names the stream it measured
        by hash; <code>rd_verify</code> re-encodes them and fails if any figure
        has moved.
      </p>
    </section>
  );
}

function ClipSection({
  campaign,
  clip,
  metric,
}: {
  campaign: Campaign;
  clip: string;
  metric: Metric;
}) {
  const curves = useMemo(
    () => campaign.curves.filter((curve) => curve.clip === clip),
    [campaign, clip],
  );
  const baseline = curves.find((curve) => curve.toolset === "full");
  const shape = curves[0];

  return (
    <article className="finding open">
      <h3>
        {clip}
        {shape ? (
          <span className="quiet">
            {" "}
            — {shape.width}×{shape.height}, {shape.frames} frames
          </span>
        ) : null}
      </h3>
      <RdChart curves={curves} metric={metric} />
      {baseline ? <AblationTable curves={curves} /> : null}
    </article>
  );
}

/// The rate–quality chart, drawn by hand on a canvas.
///
/// Hand-rolled for the same reason the picture above is: a charting library
/// would be a dependency between the reader and the measurement, and this is
/// five points and two axes. The one thing it does that a naive plot would not
/// is use a logarithmic rate axis, because rate–quality curves are read as
/// bits doubling rather than bits increasing.
function RdChart({ curves, metric }: { curves: Curve[]; metric: Metric }) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    const draw = () => {
      const ratio = window.devicePixelRatio || 1;
      const width = canvas.clientWidth || 640;
      const height = 320;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);

      const style = getComputedStyle(canvas);
      const ink = style.getPropertyValue("--ink").trim() || "#e6e9ee";
      const quiet = style.getPropertyValue("--ink-quiet").trim() || "#9aa4b2";
      const edge = style.getPropertyValue("--edge").trim() || "#262d38";

      const points = curves.flatMap((curve) => curve.points);
      if (points.length === 0) return;
      const values = (point: RatePoint) => point[metric];
      const rates = points.map((point) => Math.log(point.rate));
      const qualities = points.map(values);

      // A margin of error around the data so the outermost markers are not drawn
      // half off the plot.
      const pad = (low: number, high: number) => {
        const slack = (high - low) * 0.08 || 1;
        return [low - slack, high + slack] as const;
      };
      const [rateLow, rateHigh] = pad(Math.min(...rates), Math.max(...rates));
      const [qualityLow, qualityHigh] = pad(
        Math.min(...qualities),
        Math.max(...qualities),
      );

      const left = 62;
      const right = width - 12;
      const top = 14;
      const bottom = height - 34;
      const toX = (rate: number) =>
        left +
        ((Math.log(rate) - rateLow) / (rateHigh - rateLow)) * (right - left);
      const toY = (quality: number) =>
        bottom -
        ((quality - qualityLow) / (qualityHigh - qualityLow)) * (bottom - top);

      context.font = "11px ui-monospace, SFMono-Regular, Menlo, monospace";
      context.strokeStyle = edge;
      context.fillStyle = quiet;
      context.lineWidth = 1;

      // Horizontal rules with the quality they stand for. Five is enough to read
      // a value off and few enough not to become a grid the eye has to ignore.
      for (let step = 0; step <= 4; step += 1) {
        const quality = qualityLow + ((qualityHigh - qualityLow) * step) / 4;
        const y = Math.round(toY(quality)) + 0.5;
        context.beginPath();
        context.moveTo(left, y);
        context.lineTo(right, y);
        context.stroke();
        context.textAlign = "right";
        context.fillText(
          quality.toFixed(metric === "ssimY" ? 4 : 1),
          left - 8,
          y + 4,
        );
      }

      // The rate axis is labelled in kilobits per second at the measured points
      // rather than at round numbers, so every tick is somewhere a measurement
      // actually happened.
      const ticks = [...new Set(points.map((point) => point.rate))].sort(
        (a, b) => a - b,
      );
      context.textAlign = "center";
      for (const rate of [
        ticks[0]!,
        ticks[Math.floor(ticks.length / 2)]!,
        ticks.at(-1)!,
      ]) {
        context.fillText(
          `${(rate / 1000).toFixed(0)}k`,
          toX(rate),
          bottom + 18,
        );
      }

      context.strokeStyle = quiet;
      context.beginPath();
      context.moveTo(left, top);
      context.lineTo(left, bottom);
      context.lineTo(right, bottom);
      context.stroke();

      for (const curve of curves) {
        const colour = TOOLSET_COLOURS[curve.toolset] ?? ink;
        const ordered = [...curve.points].sort((a, b) => a.rate - b.rate);
        context.strokeStyle = colour;
        context.fillStyle = colour;
        context.lineWidth = curve.toolset === "full" ? 2.5 : 1.5;

        context.beginPath();
        ordered.forEach((point, index) => {
          const x = toX(point.rate);
          const y = toY(values(point));
          if (index === 0) context.moveTo(x, y);
          else context.lineTo(x, y);
        });
        context.stroke();

        for (const point of ordered) {
          context.beginPath();
          context.arc(toX(point.rate), toY(values(point)), 3, 0, Math.PI * 2);
          context.fill();
        }
      }

      context.fillStyle = quiet;
      context.textAlign = "center";
      context.fillText("bitrate, logarithmic", (left + right) / 2, height - 6);
    };

    draw();

    // The canvas takes its width from layout and its backing store from script,
    // so nothing redraws it when the column changes width — rotating a phone or
    // dragging a window would leave a stretched copy of the previous size. The
    // observer is what makes the chart a picture of the data rather than a
    // picture of the data at whatever width the page first loaded at.
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [curves, metric]);

  return (
    <div className="chart">
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={`Rate against ${METRIC_LABELS[metric]} for ${curves.length} toolsets`}
      />
      <ul className="legend">
        {curves.map((curve) => (
          <li key={curve.toolset}>
            <span
              className="swatch"
              style={{
                background: TOOLSET_COLOURS[curve.toolset] ?? "currentColor",
              }}
              aria-hidden="true"
            />
            {curve.toolset}
          </li>
        ))}
      </ul>
    </div>
  );
}

/// How close the average-bitrate controller lands to what it was asked for.
///
/// Kept in its own table, deliberately away from the curves above. Three
/// average-bitrate points are not a rate–distortion curve: the quality at each
/// one is an outcome of the controller rather than a setting, so fitting a
/// curve through them would produce something that looks like a rate–distortion
/// result and is a different measurement entirely. The receipt keeps them in a
/// different shape for the same reason, so nothing that reads a curve can pick
/// them up by accident.
function RateControlTable({ clips }: { clips: RateControlClip[] }) {
  if (clips.length === 0) return null;
  return (
    <article className="finding open">
      <h3>Average-bitrate accuracy</h3>
      <table className="ablations">
        <caption>
          What the rate controller delivered against what it was asked for. This is a
          single-pass leaky bucket, so the figure is a steady-state one: it is measured over
          more frames than the curves above use, because a window shorter than the
          controller&apos;s convergence time measures the transient instead of the controller.
        </caption>
        <thead>
          <tr>
            <th scope="col">Clip</th>
            <th scope="col">Target</th>
            <th scope="col">Achieved</th>
            <th scope="col">Error</th>
            <th scope="col">PSNR-Y</th>
          </tr>
        </thead>
        <tbody>
          {clips.flatMap((clip) =>
            clip.targets.map((target) => (
              <tr key={`${clip.clip}-${target.targetBps}`}>
                <th scope="row">
                  {clip.clip} <span className="quiet">({clip.frames} frames)</span>
                </th>
                <td>{(target.targetBps / 1000).toFixed(1)} kbps</td>
                <td>{(target.achievedBps / 1000).toFixed(1)} kbps</td>
                <td className={Math.abs(target.errorPercent) <= 5 ? "negative" : undefined}>
                  {target.errorPercent >= 0 ? "+" : ""}
                  {target.errorPercent.toFixed(2)}%
                </td>
                <td>{target.psnrY.toFixed(2)} dB</td>
              </tr>
            )),
          )}
        </tbody>
      </table>
    </article>
  );
}

/// What each tool is worth, as a bitrate difference against the full toolset.
///
/// Every figure here is read out of the receipt. The campaign measured it, and
/// `rd_verify` re-derives it on every run, so a number that stopped being true
/// fails the build rather than reaching this table.
function AblationTable({ curves }: { curves: Curve[] }) {
  return (
    <table className="ablations">
      <caption>
        Bitrate difference against the full toolset at equal quality. A positive
        figure means the ablation spends more bits for the same picture — which
        is what turning a working tool off should cost.
      </caption>
      <thead>
        <tr>
          <th scope="col">Toolset</th>
          <th scope="col">BD-rate</th>
          <th scope="col">What is off</th>
        </tr>
      </thead>
      <tbody>
        {curves
          .filter((curve) => curve.toolset !== "full")
          .map((curve) => {
            const result = curve.bdRate;
            return (
              <tr key={curve.toolset}>
                <th scope="row">
                  <span
                    className="swatch"
                    style={{
                      background:
                        TOOLSET_COLOURS[curve.toolset] ?? "currentColor",
                    }}
                    aria-hidden="true"
                  />
                  {curve.toolset}
                </th>
                <td
                  className={
                    result !== null && "percent" in result && result.percent < 0
                      ? "negative"
                      : undefined
                  }
                >
                  {result === null
                    ? "not recorded"
                    : "percent" in result
                      ? `${result.percent >= 0 ? "+" : ""}${result.percent.toFixed(2)}%`
                      : result.refused}
                </td>
                <td className="quiet">{TOOLSET_NOTES[curve.toolset] ?? ""}</td>
              </tr>
            );
          })}
      </tbody>
    </table>
  );
}
