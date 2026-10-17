// The page's whole state, encoded into the URL fragment.
//
// The fragment, not the query string: a fragment never reaches a server, and
// this page has no server to reach. Sharing a link is the only way to hand
// someone a specific block of a specific frame with the same overlays on, so
// the encoding stays short and stable rather than clever.
//
// Everything here is defensive. A shared link is text a stranger typed, and a
// bad field is dropped in favour of the default rather than allowed to become
// `NaN` in a canvas coordinate.

import { NO_OVERLAYS, OVERLAY_NAMES, type HeatmapBasis, type Overlays } from "./render";

export type Tab = "projection" | "cutting" | "curves";

export interface ViewState {
  streamDigest?: string;
  sourceDigest?: string;
  catalogue?: string;
  tab: Tab;
  frame: number;
  overlays: Overlays;
  basis: HeatmapBasis;
  /// The selected block's top-left luma sample, if one is selected.
  selection: [number, number] | null;
}

export const DEFAULT_VIEW: ViewState = {
  tab: "projection",
  frame: 0,
  overlays: { ...NO_OVERLAYS, partition: true },
  basis: "modeled",
  selection: null,
};

/// Overlays travel as one string of initials, so a link with all six on stays
/// shorter than a line of prose.
function encodeOverlays(overlays: Overlays): string {
  return OVERLAY_NAMES.filter((name) => overlays[name])
    .map((name) => name[0] ?? "")
    .join("");
}

function decodeOverlays(value: string): Overlays {
  const overlays = { ...NO_OVERLAYS };
  for (const name of OVERLAY_NAMES) {
    if (value.includes(name[0] ?? "")) overlays[name] = true;
  }
  return overlays;
}

export function encodeView(view: ViewState): string {
  const parts = [`f=${view.frame}`, `o=${encodeOverlays(view.overlays)}`, `h=${view.basis}`];
  if (view.selection) parts.push(`b=${view.selection[0]},${view.selection[1]}`);
  if (view.tab !== DEFAULT_VIEW.tab) parts.push(`t=${view.tab}`);
  if (view.streamDigest) parts.push(`s=${view.streamDigest}`);
  if (view.sourceDigest) parts.push(`y=${view.sourceDigest}`);
  if (view.catalogue) parts.push(`c=${encodeURIComponent(view.catalogue)}`);
  return parts.join("&");
}

export function decodeView(fragment: string): ViewState {
  const parameters = new URLSearchParams(fragment.replace(/^#/, ""));
  const view: ViewState = {
    tab: DEFAULT_VIEW.tab,
    frame: DEFAULT_VIEW.frame,
    overlays: { ...DEFAULT_VIEW.overlays },
    basis: DEFAULT_VIEW.basis,
    selection: null,
  };

  const bound = parameters.has("s") || parameters.has("y") || parameters.has("c");
  if (bound) {
    for (const key of parameters.keys()) {
      if (parameters.getAll(key).length !== 1 || !["s", "y", "c", "f", "o", "h", "b", "t"].includes(key)) throw new Error("invalid bookmark fields");
    }
    for (const [key, field] of [["s", "streamDigest"], ["y", "sourceDigest"]] as const) {
      const digest = parameters.get(key);
      if (digest !== null) {
        if (!/^[0-9a-fA-F]{64}$/.test(digest)) throw new Error("invalid bookmark digest");
        view[field] = digest.toLowerCase();
      }
    }
    if (!view.streamDigest) throw new Error("a bound bookmark needs a stream digest");
    const catalogue = parameters.get("c");
    if (catalogue !== null) view.catalogue = catalogue;
    const integer = (value: string) => /^[0-9]+$/.test(value) && Number.isSafeInteger(Number(value));
    const frame = parameters.get("f");
    if (frame !== null && !integer(frame)) throw new Error("invalid bookmark frame");
    const selection = parameters.get("b");
    if (selection !== null && (selection.split(",").length !== 2 || !selection.split(",").every(integer))) throw new Error("invalid bookmark block");
    const overlays = parameters.get("o");
    if (overlays !== null && (!overlays.split("").every((initial) => OVERLAY_NAMES.some((name) => name[0] === initial)) || new Set(overlays).size !== overlays.length)) throw new Error("invalid bookmark overlays");
    const basis = parameters.get("h");
    if (basis !== null && !["modeled", "emitted"].includes(basis)) throw new Error("invalid bookmark basis");
    const tab = parameters.get("t");
    if (tab !== null && !["projection", "cutting", "curves"].includes(tab)) throw new Error("invalid bookmark tab");
  }
  const tab = parameters.get("t");
  if (tab === "projection" || tab === "cutting" || tab === "curves") view.tab = tab;

  const frame = Number.parseInt(parameters.get("f") ?? "", 10);
  if (Number.isInteger(frame) && frame >= 0) view.frame = frame;

  const overlays = parameters.get("o");
  if (overlays !== null) view.overlays = decodeOverlays(overlays);

  const basis = parameters.get("h");
  if (basis === "modeled" || basis === "emitted") view.basis = basis;

  const selection = parameters.get("b");
  if (selection) {
    const [rawX, rawY] = selection.split(",");
    const x = Number.parseInt(rawX ?? "", 10);
    const y = Number.parseInt(rawY ?? "", 10);
    if (Number.isInteger(x) && Number.isInteger(y) && x >= 0 && y >= 0) {
      view.selection = [x, y];
    }
  }

  return view;
}

/// Writes the state into the address bar without adding a history entry.
///
/// Scrubbing a clip would otherwise bury the back button under one entry per
/// frame, which makes leaving the page harder the longer someone stays.
export function writeView(view: ViewState): void {
  const fragment = `#${encodeView(view)}`;
  if (window.location.hash !== fragment) {
    window.history.replaceState(null, "", fragment);
  }
}
