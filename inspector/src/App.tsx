import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Decoder, type DecodedFrame, type StreamInfo } from "./decoder";
import { BlockPanel } from "./BlockPanel";
import { CuttingRoom } from "./CuttingRoom";
import { type CodingBlock, type FrameReport, blockAt, parseReport } from "./probe";
import {
  OVERLAY_LABELS,
  OVERLAY_NAMES,
  type HeatmapBasis,
  drawOverlays,
  frameToImageData,
} from "./render";
import { type ViewState, decodeView, writeView } from "./share";

// Both are produced by `scripts/build-inspector.sh` into `public/`. The sample
// is a real committed conformance stream, decoded live like any other: there is
// no pre-decoded fixture anywhere in this application, because a picture that
// did not come from the decoder proves nothing about the decoder.
const MODULE_URL = "./kf_wasm.wasm";
const SAMPLE_URL = "./sample.kfv";

type Loading = { phase: "loading" } | { phase: "ready" } | { phase: "failed"; error: string };

export function App() {
  const [status, setStatus] = useState<Loading>({ phase: "loading" });
  const [decoder, setDecoder] = useState<Decoder | null>(null);
  const [info, setInfo] = useState<StreamInfo | null>(null);
  const [source, setSource] = useState("sample clip");
  const [view, setView] = useState<ViewState>(() => decodeView(window.location.hash));
  const [frame, setFrame] = useState<DecodedFrame | null>(null);
  const [report, setReport] = useState<FrameReport | null>(null);
  const [frameError, setFrameError] = useState<string | null>(null);

  const pictureRef = useRef<HTMLCanvasElement | null>(null);
  const overlayRef = useRef<HTMLCanvasElement | null>(null);

  const openStream = useCallback(
    (loaded: Decoder, bytes: Uint8Array, label: string) => {
      try {
        const opened = loaded.open(bytes);
        setInfo(opened);
        setSource(label);
        setFrameError(null);
        // The frame is clamped because a shared link may name one this stream
        // does not have. The selection is left alone: clearing it here would
        // throw away the block a shared link asked for, since opening the
        // stream is the first thing that happens on load. Callers that mean to
        // start fresh clear it themselves.
        setView((current) => ({
          ...current,
          frame: Math.min(current.frame, opened.frameCount - 1),
        }));
      } catch (error) {
        setInfo(null);
        setFrame(null);
        setReport(null);
        setFrameError(error instanceof Error ? error.message : String(error));
      }
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const loaded = await Decoder.load(MODULE_URL);
        const response = await fetch(SAMPLE_URL);
        if (!response.ok) throw new Error(`the sample clip did not load (${response.status})`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (cancelled) return;
        setDecoder(loaded);
        openStream(loaded, bytes, "sample clip");
        setStatus({ phase: "ready" });
      } catch (error) {
        if (cancelled) return;
        setStatus({
          phase: "failed",
          error: error instanceof Error ? error.message : String(error),
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [openStream]);

  // Decode and probe whenever the requested frame changes. Both are synchronous
  // calls into the module; the frames here are small enough that yielding
  // between them would cost more than it saves.
  useEffect(() => {
    if (!decoder || !info) return;
    const index = Math.min(Math.max(view.frame, 0), info.frameCount - 1);
    try {
      setFrame(decoder.decodeFrame(index));
      setReport(parseReport(decoder.probeFrame(index)));
      setFrameError(null);
    } catch (error) {
      setFrame(null);
      setReport(null);
      setFrameError(error instanceof Error ? error.message : String(error));
    }
  }, [decoder, info, view.frame]);

  useEffect(() => writeView(view), [view]);

  const selected = useMemo<CodingBlock | null>(() => {
    if (!report || !view.selection) return null;
    return blockAt(report, view.selection[0], view.selection[1]);
  }, [report, view.selection]);

  // One integer scale, so a 64-wide clip is inspectable without the browser
  // inventing pixels between the ones the decoder produced.
  const scale = useMemo(() => {
    if (!info) return 1;
    return Math.max(1, Math.min(8, Math.floor(768 / Math.max(info.width, 1))));
  }, [info]);

  useEffect(() => {
    const canvas = pictureRef.current;
    if (!canvas || !frame) return;
    canvas.width = frame.width;
    canvas.height = frame.height;
    const context = canvas.getContext("2d");
    context?.putImageData(frameToImageData(frame), 0, 0);
  }, [frame]);

  useEffect(() => {
    const canvas = overlayRef.current;
    if (!canvas || !report || !info) return;
    canvas.width = info.width * scale;
    canvas.height = info.height * scale;
    drawOverlays(canvas, report, view.overlays, view.basis, selected, scale);
  }, [report, info, view.overlays, view.basis, selected, scale]);

  const onPickBlock = (event: React.MouseEvent<HTMLCanvasElement>) => {
    if (!info) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = Math.floor(((event.clientX - bounds.left) / bounds.width) * info.width);
    const y = Math.floor(((event.clientY - bounds.top) / bounds.height) * info.height);
    setView((current) => ({ ...current, selection: [x, y] }));
  };

  const onPickFile = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file || !decoder) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    setView((current) => ({ ...current, frame: 0, selection: null }));
    openStream(decoder, bytes, file.name);
  };

  /// Opens a pinned regression stream from the catalogue and switches back to
  /// the picture, so a finding can be looked at rather than only read about.
  const onOpenStream = useCallback(
    async (url: string, label: string) => {
      if (!decoder) return;
      try {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`${label} did not load (${response.status})`);
        openStream(decoder, new Uint8Array(await response.arrayBuffer()), label);
        setView((current) => ({ ...current, tab: "projection", frame: 0, selection: null }));
      } catch (error) {
        setFrameError(error instanceof Error ? error.message : String(error));
        setView((current) => ({ ...current, tab: "projection" }));
      }
    },
    [decoder, openStream],
  );

  if (status.phase === "loading") {
    return <main className="shell">Loading the decoder…</main>;
  }
  if (status.phase === "failed") {
    return (
      <main className="shell">
        <h1>Projection room</h1>
        <p className="error" role="alert">
          {status.error}
        </p>
      </main>
    );
  }

  return (
    <main className="shell">
      <header className="masthead">
        <h1>Projection room</h1>
        <p className="lede">
          Every pixel and every number below came out of the decoder in this tab, compiled to
          WebAssembly from the same source the conformance suite runs natively.
        </p>
      </header>

      <nav className="tabs" aria-label="Sections">
        {(
          [
            ["projection", "Projection room"],
            ["cutting", "Cutting room"],
          ] as const
        ).map(([tab, label]) => (
          <button
            key={tab}
            type="button"
            className={view.tab === tab ? "tab current" : "tab"}
            aria-current={view.tab === tab ? "page" : undefined}
            onClick={() => setView((current) => ({ ...current, tab }))}
          >
            {label}
          </button>
        ))}
      </nav>

      {view.tab === "cutting" ? <CuttingRoom onOpenStream={onOpenStream} /> : null}

      <section className="stage" hidden={view.tab !== "projection"}>
        <div className="viewport" style={{ width: (info?.width ?? 0) * scale }}>
          <canvas
            ref={pictureRef}
            className="picture"
            style={{ width: (info?.width ?? 0) * scale, height: (info?.height ?? 0) * scale }}
          />
          <canvas
            ref={overlayRef}
            className="overlay"
            onClick={onPickBlock}
            aria-label="Decoded frame with syntax overlays; click a block for its syntax"
          />
        </div>

        <aside className="side">
          <StreamFacts info={info} source={source} report={report} frame={frame} />
          <OverlayControls view={view} setView={setView} />
          <BlockPanel report={report} block={selected} />
        </aside>
      </section>

      {frameError && view.tab === "projection" ? (
        <p className="error" role="alert">
          {frameError}
        </p>
      ) : null}

      {view.tab === "projection" ? <Scrubber info={info} view={view} setView={setView} /> : null}

      <footer className="tools" hidden={view.tab !== "projection"}>
        <label className="file">
          Open a stream
          <input type="file" accept=".kfv" onChange={onPickFile} />
        </label>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard?.writeText(window.location.href);
          }}
        >
          Copy link to this view
        </button>
      </footer>
    </main>
  );
}

function StreamFacts({
  info,
  source,
  report,
  frame,
}: {
  info: StreamInfo | null;
  source: string;
  report: FrameReport | null;
  frame: DecodedFrame | null;
}) {
  if (!info) return <div className="card">No stream is open.</div>;
  return (
    <div className="card">
      <h2>Stream</h2>
      <dl>
        <dt>Source</dt>
        <dd>{source}</dd>
        <dt>Dimensions</dt>
        <dd>
          {info.width}×{info.height}
        </dd>
        <dt>Frames</dt>
        <dd>
          {info.frameCount} at {info.fpsNum}/{info.fpsDen} fps
        </dd>
        <dt>Keyframes</dt>
        <dd>{info.keyframes.join(", ")}</dd>
        {frame ? (
          <>
            <dt>This frame cost</dt>
            <dd>
              {frame.entryCost} decode{frame.entryCost === 1 ? "" : "s"} from keyframe{" "}
              {frame.entryKeyframe}
            </dd>
          </>
        ) : null}
        {report ? (
          <>
            <dt>Payload</dt>
            <dd>
              {report.inputPayloadLen} bytes, {report.frameFlushBytes} of them the frame flush
            </dd>
          </>
        ) : null}
      </dl>
      {report ? <ReplayBadge report={report} /> : null}
    </div>
  );
}

/// The badge that keeps the accounting honest.
///
/// When the input payload is byte-identical to the canonical replay, the timing
/// buckets add up to the file. When it is not — which a decodable stream is
/// allowed to be — the buckets still describe the replay, and no unmatched
/// input byte is ever coloured as if it belonged to a block.
function ReplayBadge({ report }: { report: FrameReport }) {
  if (report.canonicalPayloadMatch) {
    return (
      <p className="badge match">
        input bytes = canonical replay ({report.canonicalReplayPayloadLen} bytes)
      </p>
    );
  }
  return (
    <p className="badge mismatch">
      noncanonical input: showing canonical replay timing. Input is {report.inputPayloadLen} bytes,
      the replay is {report.canonicalReplayPayloadLen}; they first differ at offset{" "}
      {report.firstMismatchOffset ?? "unknown"}. Unmatched input bytes are not attributed to any
      block.
    </p>
  );
}

function OverlayControls({
  view,
  setView,
}: {
  view: ViewState;
  setView: React.Dispatch<React.SetStateAction<ViewState>>;
}) {
  return (
    <div className="card">
      <h2>Overlays</h2>
      <ul className="toggles">
        {OVERLAY_NAMES.map((name) => (
          <li key={name}>
            <label>
              <input
                type="checkbox"
                checked={view.overlays[name]}
                onChange={() =>
                  setView((current) => ({
                    ...current,
                    overlays: { ...current.overlays, [name]: !current.overlays[name] },
                  }))
                }
              />
              {OVERLAY_LABELS[name]}
            </label>
          </li>
        ))}
      </ul>
      <fieldset className="basis">
        <legend>Heatmap shows</legend>
        {(
          [
            ["modeled", "Modeled entropy"],
            ["emitted", "Emission-time bytes"],
          ] as [HeatmapBasis, string][]
        ).map(([value, label]) => (
          <label key={value}>
            <input
              type="radio"
              name="basis"
              checked={view.basis === value}
              onChange={() => setView((current) => ({ ...current, basis: value }))}
            />
            {label}
          </label>
        ))}
      </fieldset>
      <p className="disclaimer">
        Neither figure is the number of bits a block occupies. Modeled entropy is what the
        encoder&apos;s cost model predicted; emission-time bytes is when the arithmetic coder
        happened to flush, and delayed carry means a byte can be emitted while a later block is
        being coded. No block owns bits in an adaptive arithmetic coder.
      </p>
    </div>
  );
}

function Scrubber({
  info,
  view,
  setView,
}: {
  info: StreamInfo | null;
  view: ViewState;
  setView: React.Dispatch<React.SetStateAction<ViewState>>;
}) {
  if (!info) return null;
  const last = Math.max(0, info.frameCount - 1);
  return (
    <section className="scrub">
      <div className="ticks" aria-hidden="true">
        {info.keyframes.map((keyframe) => (
          <span
            key={keyframe}
            className="tick"
            style={{ left: `${last === 0 ? 0 : (keyframe / last) * 100}%` }}
          />
        ))}
      </div>
      <label>
        Frame {view.frame} of {last}
        <input
          type="range"
          min={0}
          max={last}
          step={1}
          value={Math.min(view.frame, last)}
          onChange={(event) =>
            setView((current) => ({
              ...current,
              frame: Number.parseInt(event.target.value, 10),
              selection: null,
            }))
          }
        />
      </label>
    </section>
  );
}
