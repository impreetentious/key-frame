import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  Decoder,
  type DecodedFrame,
  type PacketOutcome,
  type StreamInfo,
} from "./decoder";
import { BlockPanel } from "./BlockPanel";
import { CuttingRoom, parseCatalogue } from "./CuttingRoom";
import { Curves } from "./Curves";
import { type CodingBlock, type FrameReport, allBlocks, blockAt, parseReport } from "./probe";
import {
  OVERLAY_LABELS,
  OVERLAY_NAMES,
  type HeatmapBasis,
  drawOverlays,
  frameToImageData,
} from "./render";
import { type ViewState, DEFAULT_VIEW, decodeView, writeView } from "./share";
import { type SourceClip, type WorstBlock, parseY4m, worstBlock } from "./source";

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
  const [incoming] = useState(() => {
    try { const view = decodeView(window.location.hash); return { view, error: "", legacy: window.location.hash.length > 1 && !view.streamDigest }; }
    catch (error) { return { view: DEFAULT_VIEW, error: String(error), legacy: false }; }
  });
  const [view, setView] = useState<ViewState>(incoming.view);
  const invalidIncoming = useRef(incoming.error);
  const firstOpen = useRef(true);
  const pending = useRef<ViewState | null>(incoming.view.streamDigest ? incoming.view : null);
  const requiredSourceDigest = useRef(incoming.view.sourceDigest);
  const [bookmarkStatus, setBookmarkStatus] = useState(incoming.error || (pending.current ? "Load the local file matching this bookmark." : "Unbound view bookmark"));
  const [pendingBookmark, setPendingBookmark] = useState(Boolean(pending.current) || Boolean(incoming.error));
  const generation = useRef({ stream: 0, source: 0 });
  const [blockIndex, setBlockIndex] = useState("");
  const [blockError, setBlockError] = useState("");
  const [copyStatus, setCopyStatus] = useState("");
  const [frame, setFrame] = useState<DecodedFrame | null>(null);
  const [report, setReport] = useState<FrameReport | null>(null);
  const [frameError, setFrameError] = useState<string | null>(null);
  // The original pictures, if the visitor has supplied them. Without these the
  // page can show what the decoder produced but not how wrong it is, and it
  // says so rather than estimating.
  const [sourceClip, setSourceClip] = useState<SourceClip | null>(null);
  const [worst, setWorst] = useState<WorstBlock | null>(null);
  // A stream the strict decoder refused, kept so the visitor can ask what the
  // corruption rules make of it. Holding the bytes is the whole feature: the
  // rules are the most carefully specified part of this format and the only
  // part nobody could previously watch run.
  const [refused, setRefused] = useState<Uint8Array | null>(null);
  const [recovery, setRecovery] = useState<PacketOutcome[] | null>(null);

  const pictureRef = useRef<HTMLCanvasElement | null>(null);
  const overlayRef = useRef<HTMLCanvasElement | null>(null);

  const beginStream = useCallback(() => {
    generation.current.source += 1;
    requiredSourceDigest.current = pending.current?.sourceDigest;
    setSourceClip(null);
    setWorst(null);
    setInfo(null);
    setView((current) => { const next = { ...current }; delete next.streamDigest; delete next.sourceDigest; delete next.catalogue; return next; });
    setFrame(null);
    setReport(null);
    return ++generation.current.stream;
  }, []);
  const hashBytes = async (bytes: Uint8Array): Promise<string> => {
    if (!crypto.subtle) throw new Error("Web Crypto is unavailable; bound bookmarks require SHA-256");
    return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes.slice().buffer))].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  };
  const openStream = useCallback(
    async (loaded: Decoder, bytes: Uint8Array, label: string, ticket: number, catalogue?: string) => {
      try {
        let digest: string | undefined;
        if (crypto.subtle) digest = await hashBytes(bytes);
        if (ticket !== generation.current.stream) return;
        if (invalidIncoming.current) return;
        const requested = pending.current;
        if (requested && !digest) { setBookmarkStatus("Web Crypto is unavailable; discard the requested bookmark for local inspection."); return; }
        if (requested && requested.streamDigest !== digest) {
          setBookmarkStatus("This file does not match the bookmark. Load the local matching KFV file.");
          return;
        }
        const opened = loaded.open(bytes);
        if (requested) {
          if (requested.frame >= opened.frameCount) throw new Error("invalid bookmark: frame is outside this stream");
          if (requested.selection) {
            const blocks = allBlocks(parseReport(loaded.probeFrame(requested.frame)));
            if (!blocks.some((block) => block.x === requested.selection?.[0] && block.y === requested.selection?.[1])) throw new Error("invalid bookmark: no coding block at these coordinates");
          }
        }
        setInfo(opened);
        setSource(label);
        setFrameError(null);
        setRefused(null);
        setRecovery(null);
        requiredSourceDigest.current = requested?.sourceDigest;
        const initial = firstOpen.current;
        setView((current) => ({ ...(requested ?? current), frame: requested ? requested.frame : (initial ? Math.min(current.frame, opened.frameCount - 1) : 0), selection: requested ? requested.selection : (initial ? current.selection : null), streamDigest: digest, sourceDigest: requested?.sourceDigest, catalogue } as ViewState));
        firstOpen.current = false;
        pending.current = null;
        setPendingBookmark(false);
        setBookmarkStatus(requested?.sourceDigest ? "Stream matched. Load the matching Y4M before source-error analysis. A match does not prove encoder provenance." : (digest ? (initial && incoming.legacy ? "Unbound view bookmark loaded; new links bind the loaded stream." : "Stream-bound bookmark") : "Web Crypto unavailable; local inspection only"));
      } catch (error) {
        if (ticket !== generation.current.stream) return;
        if (pending.current) setBookmarkStatus(String(error));
        else { setInfo(null); setFrame(null); setReport(null); setRecovery(null); setRefused(bytes); setFrameError(String(error)); }
      }
    }, []);

  /// Walks the refused stream to its end and shows what the rules did.
  const onWalkAnyway = useCallback(() => {
    if (!decoder || !refused) return;
    try {
      const opened = decoder.openTolerant(refused);
      setInfo(opened);
      setRecovery(decoder.recovery());
      setSource("damaged stream, walked");
      setFrameError(null);
      setRefused(null);
      setView((current) => ({ ...current, frame: 0, selection: null }));
    } catch (error) {
      setFrameError(error instanceof Error ? error.message : String(error));
    }
  }, [decoder, refused]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const loaded = await Decoder.load(MODULE_URL);
        let streamUrl = SAMPLE_URL;
        let catalogue = "sample";
        if (pending.current?.catalogue && pending.current.catalogue !== "sample") {
          const listing = await fetch("./cutting-room.json");
          const entries = parseCatalogue(await listing.json());
          const entry = entries.find((entry) => entry.id === pending.current?.catalogue);
          if (entry && /^\.\/regressions\/[^/]+\.kfv$/.test(entry.stream)) { streamUrl = entry.stream; catalogue = entry.id; }
        }
        const response = await fetch(streamUrl);
        if (!response.ok) throw new Error(`the sample clip did not load (${response.status})`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (cancelled) return;
        setDecoder(loaded);
        await openStream(loaded, bytes, catalogue === "sample" ? "sample clip" : catalogue, beginStream(), catalogue);
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
      generation.current.stream += 1;
      generation.current.source += 1;
    };
  }, [openStream, beginStream]);

  // Decode and probe whenever the requested frame changes. Both are synchronous
  // calls into the module; the frames here are small enough that yielding
  // between them would cost more than it saves.
  useEffect(() => {
    if (!decoder || !info) return;
    const index = Math.min(Math.max(view.frame, 0), info.frameCount - 1);
    try {
      setFrame(decoder.decodeFrame(index));
      // A walked stream is a timeline indexed by packet; the syntax report is
      // indexed by the frame index a packet declares, and on a damaged stream
      // those are different numbers. The module refuses to answer, correctly,
      // so the page does not ask — and says why on the page rather than
      // leaving the overlays mysteriously empty.
      setReport(recovery ? null : parseReport(decoder.probeFrame(index)));
      setFrameError(null);
    } catch (error) {
      setFrame(null);
      setReport(null);
      setFrameError(error instanceof Error ? error.message : String(error));
    }
  }, [decoder, info, view.frame, recovery]);

  useEffect(() => { if (!pendingBookmark) writeView(view); }, [view, pendingBookmark]);

  const selected = useMemo<CodingBlock | null>(() => {
    if (!report || !view.selection) return null;
    return allBlocks(report).find((block) => block.x === view.selection?.[0] && block.y === view.selection?.[1]) ?? null;
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
    if (!info || !report) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = Math.floor(((event.clientX - bounds.left) / bounds.width) * info.width);
    const y = Math.floor(((event.clientY - bounds.top) / bounds.height) * info.height);
    const block = blockAt(report, x, y);
    if (block) setView((current) => ({ ...current, selection: [block.x, block.y] }));
  };

  const onPickFile = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file || !decoder) return;
    const ticket = beginStream();
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      if (ticket === generation.current.stream) await openStream(decoder, bytes, file.name, ticket);
    } catch (error) { if (ticket === generation.current.stream) setFrameError(String(error)); }
  };

  /// Loads the original clip a stream was encoded from.
  ///
  /// Refused unless it matches the stream in dimensions and frame count. A clip
  /// that merely parses would produce an error map computed against the wrong
  /// pictures, which is the one failure worse than not answering at all.
  const onPickSource = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file || !info || recovery) return;
    const streamTicket = generation.current.stream;
    const sourceTicket = ++generation.current.source;
    const current = () => streamTicket === generation.current.stream && sourceTicket === generation.current.source;
    setSourceClip(null); setWorst(null);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const digest = crypto.subtle ? await hashBytes(bytes) : undefined;
      if (!current()) return;
      if (requiredSourceDigest.current && digest !== requiredSourceDigest.current) throw new Error("This Y4M does not match the bookmark source");
      const clip = parseY4m(bytes);
      if (!info) throw new Error("open a stream before its source");
      if (clip.width !== info.width || clip.height !== info.height) {
        throw new Error(
          `the source is ${clip.width}x${clip.height} and the stream is ${info.width}x${info.height}`,
        );
      }
      if (clip.frameCount < info.frameCount) {
        throw new Error(
          `the source has ${clip.frameCount} frames and the stream has ${info.frameCount}`,
        );
      }
      setSourceClip(clip);
      setView((current) => ({ ...current, sourceDigest: digest } as ViewState));
      setBookmarkStatus("Compatible supplied source loaded; this does not prove it was the encoder original.");
      setFrameError(null);
    } catch (error) {
      if (!current()) return;
      setSourceClip(null);
      setWorst(null);
      setFrameError(error instanceof Error ? error.message : String(error));
    }
  };

  /// Selects the block that went worst: the most error for the fewest bits.
  const onFindWorst = () => {
    const plane = sourceClip?.luma[view.frame];
    if (!plane || !report || !frame) return;
    let found: WorstBlock | null;
    try { found = worstBlock(report, frame, plane); } catch (error) { setFrameError(String(error)); return; }
    setWorst(found);
    if (found) {
      setView((current) => ({ ...current, selection: [found.block.x, found.block.y] }));
    }
  };

  // The worst block is a fact about one frame. Carrying it across a scrub would
  // leave a figure on screen describing a picture nobody is looking at.
  useEffect(() => {
    setWorst(null);
  }, [view.frame]);

  /// Opens a pinned regression stream from the catalogue and switches back to
  /// the picture, so a finding can be looked at rather than only read about.
  const onOpenStream = useCallback(
    async (url: string, label: string) => {
      if (!decoder) return;
      const ticket = beginStream();
      try {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`${label} did not load (${response.status})`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (ticket !== generation.current.stream) return;
        const listing = await fetch("./cutting-room.json");
        const entries = parseCatalogue(await listing.json());
        const identity = entries.find((entry) => entry.stream === url)?.id;
        if (ticket !== generation.current.stream) return;
        await openStream(decoder, bytes, label, ticket, identity);
        if (!pending.current) setView((current) => ({ ...current, tab: "projection" }));
      } catch (error) {
        if (ticket !== generation.current.stream) return;
        setFrameError(error instanceof Error ? error.message : String(error));
        setView((current) => ({ ...current, tab: "projection" }));
      }
    },
    [decoder, openStream, beginStream],
  );

  const orderedBlocks = useMemo(() => report ? allBlocks(report).map((block, order) => ({ block, order })).sort((a,b) => a.block.y-b.block.y || a.block.x-b.block.x || a.block.size-b.block.size || a.order-b.order).map(({block}) => block) : [], [report]);
  const selectedIndex = selected ? orderedBlocks.indexOf(selected) : -1;
  useEffect(() => { setBlockIndex(selectedIndex < 0 ? "" : String(selectedIndex + 1)); setBlockError(""); }, [selectedIndex, report]);
  const selectIndex = (index: number) => {
    const block = orderedBlocks[index];
    if (block) { setView((current) => ({ ...current, selection: [block.x, block.y] })); setBlockIndex(String(index + 1)); setBlockError(""); }
  };
  const commitIndex = () => {
    if (!/^[0-9]+$/.test(blockIndex) || !Number.isSafeInteger(Number(blockIndex)) || Number(blockIndex) < 1 || Number(blockIndex) > orderedBlocks.length) { setBlockError("Enter a whole block index within the displayed count"); return; }
    selectIndex(Number(blockIndex) - 1);
  };

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

      <p role="status">{bookmarkStatus}</p>
      {pendingBookmark && <button type="button" onClick={() => { pending.current = null; invalidIncoming.current = ""; setPendingBookmark(false); setBookmarkStatus("Unbound view bookmark"); }}>Discard requested bookmark</button>}
      <nav className="tabs" aria-label="Sections">
        {(
          [
            ["projection", "Projection room"],
            ["cutting", "Cutting room"],
            ["curves", "Rate and distortion"],
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
      {view.tab === "curves" ? <Curves /> : null}

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
          <div className="card">
            <h2>Block Navigation</h2>
            <button type="button" disabled={!orderedBlocks.length || selectedIndex === 0} onClick={() => selectIndex(selectedIndex < 0 ? 0 : selectedIndex - 1)}>Previous block</button>
            <button type="button" disabled={!orderedBlocks.length || selectedIndex === orderedBlocks.length - 1} onClick={() => selectIndex(selectedIndex < 0 ? 0 : selectedIndex + 1)}>Next block</button>
            <label>Block index (1 to {orderedBlocks.length})<input aria-label="Block index" type="text" inputMode="numeric" disabled={!orderedBlocks.length} value={blockIndex} aria-invalid={Boolean(blockError)} onChange={(event) => setBlockIndex(event.target.value)} onBlur={() => { if (blockIndex !== "") commitIndex(); }} onKeyDown={(event) => { if (event.key === "Enter") commitIndex(); if (event.key === "Escape") { setBlockIndex(selectedIndex < 0 ? "" : String(selectedIndex + 1)); setBlockError(""); } }} /></label>
            {blockError && <p role="alert">{blockError}</p>}
            <p role="status" aria-live="polite">{selected ? `Block ${selectedIndex + 1}/${orderedBlocks.length}: (${selected.x}, ${selected.y}), ${selected.size}×${selected.size}` : "No block selected"}</p>
          </div>
          <BlockPanel report={report} block={selected} worst={worst} />
        </aside>
      </section>

      {frameError && view.tab === "projection" ? (
        <p className="error" role="alert">
          {frameError}
          {refused ? (
            <>
              {" "}
              <button type="button" className="inline" onClick={onWalkAnyway}>
                Walk it anyway and show what the rules do
              </button>
            </>
          ) : null}
        </p>
      ) : null}

      {recovery && view.tab === "projection" ? (
        <RecoveryStrip outcomes={recovery} view={view} setView={setView} />
      ) : null}

      {view.tab === "projection" ? <Scrubber info={info} view={view} setView={setView} /> : null}

      <footer className="tools" hidden={view.tab !== "projection"}>
        <label className="file">
          Open a stream
          <input type="file" accept=".kfv" onChange={onPickFile} />
        </label>
        <label className="file">
          {sourceClip ? "Source loaded" : "Load the source clip"}
          <input type="file" accept=".y4m" disabled={!info || Boolean(recovery)} onChange={onPickSource} />
        </label>
        <button
          type="button"
          onClick={onFindWorst}
          disabled={!sourceClip || !report || !frame}
          title={
            sourceClip
              ? "Select the block with the most error for the fewest bits"
              : "Load the original Y4M this stream was encoded from. Without it the page cannot measure error, and it will not guess."
          }
        >
          Why is this block ugly?
        </button>
        <button
          type="button"
          disabled={pendingBookmark || !view.streamDigest}
          onClick={() => {
            void navigator.clipboard.writeText(window.location.href).then(() => setCopyStatus("Copied"), () => setCopyStatus("Copy the address bar link manually"));
          }}
        >
          Copy link to this view
        </button>
        <span role="status">{copyStatus}</span>
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
            }))
          }
        />
      </label>
    </section>
  );
}

/// What the corruption rules made of each packet, one cell per packet.
///
/// This is the part of the format that is specified most carefully and watched
/// least: thirteen named traps drive it in both decoders, and until now none of
/// that was visible to anyone reading the page. A cell is a packet, its colour
/// is its outcome, and clicking it scrubs to the picture that packet produced —
/// which for a lost one is the last image that decoded, held, because that is
/// what the document says a decoder shows.
function RecoveryStrip({
  outcomes,
  view,
  setView,
}: {
  outcomes: PacketOutcome[];
  view: ViewState;
  setView: React.Dispatch<React.SetStateAction<ViewState>>;
}) {
  const shown = outcomes.filter((outcome) => outcome === "shown").length;
  const lost = outcomes.length - shown;
  return (
    <section className="recovery">
      <h2>What the rules did</h2>
      <p className="lede">
        This stream did not decode cleanly, so it was walked to its end instead of
        abandoned at the first fault. {outcomes.length} packet(s): {shown} decoded,{" "}
        {lost} did not. A packet that produced no image holds the last one that did — for
        display only. Nothing here is reconstructed from damaged bytes, and nothing damaged
        reached a later frame.
      </p>
      <ol className="packets">
        {outcomes.map((outcome, position) => (
          <li key={position}>
            <button
              type="button"
              className={`packet ${outcome.split(" ")[0]}`}
              aria-current={view.frame === position ? "true" : undefined}
              title={`packet ${position}: ${outcome}`}
              onClick={() => setView((current) => ({ ...current, frame: position }))}
            >
              <span className="visually-hidden">
                packet {position}: {outcome}
              </span>
            </button>
          </li>
        ))}
      </ol>
      <p className="quiet">
        Syntax overlays are off for a walked stream: the timeline is indexed by packet and
        the syntax report by the frame index a packet declares, and on a damaged stream
        those are different numbers. Reopen it strictly to inspect syntax.
      </p>
    </section>
  );
}
