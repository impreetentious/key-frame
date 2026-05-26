import { expect, test } from "@playwright/test";

import { parseCatalogue } from "../src/CuttingRoom";
import { SOURCE_LIMITS, SourceError, parseY4m } from "../src/source";
import { ReportError, blockAt, parseReport } from "../src/probe";
import { OVERLAY_NAMES } from "../src/render";
import { DEFAULT_VIEW, decodeView, encodeView, type ViewState } from "../src/share";

// The two pure readers on this page, exercised directly.
//
// Everything else in this suite drives a browser, which is the right way to
// test a page and the wrong way to test a parser: a browser run can only reach
// the inputs the decoder actually produces, and both of these exist to refuse
// inputs it never would. A shared link is text a stranger typed, and the probe
// report crosses the WebAssembly boundary as untyped JSON.
//
// No page, no fixture, no server. These import the modules and call them.

/// A report with one superblock and one block, valid in every field.
function report(overrides: Record<string, unknown> = {}): unknown {
  return {
    probe_version: 1,
    frame: {
      frame_index: 0,
      flags: { key: true, golden_refresh: false, show: true },
      qp: 32,
      input_payload_len: 10,
      canonical_replay_payload_len: 10,
      canonical_payload_match: true,
      first_mismatch_offset: null,
      frame_flush_bytes: 2,
      superblocks: [
        {
          pos: [0, 0],
          size: 64,
          structure_modeled_entropy_q16: 65536,
          structure_emitted_payload_bytes: 1,
          cbs: [
            {
              pos: [0, 0],
              size: 32,
              prediction: { kind: "intra", mode: "dc" },
              qp: 32,
              modeled_entropy_q16: 65536,
              emitted_payload_bytes: 1,
              dc_energy: 4,
              ...(overrides["block"] as Record<string, unknown> | undefined),
            },
          ],
        },
      ],
      ...(overrides["frame"] as Record<string, unknown> | undefined),
    },
  };
}

test("a well-formed report parses into blocks the overlays can find", () => {
  const parsed = parseReport(report());
  expect(parsed.superblocks).toHaveLength(1);
  expect(blockAt(parsed, 4, 4)?.size).toBe(32);
  expect(blockAt(parsed, 40, 4)).toBeNull();
});

test("a non-finite position is refused rather than drawn", () => {
  // `typeof NaN` is `"number"`, so a check that only asked for a number let a
  // position through into a canvas coordinate, where it matches no sample and
  // draws nothing — which reads as a block that cost nothing rather than as a
  // report the page could not trust.
  for (const position of [[Number.NaN, 0], [0, Number.POSITIVE_INFINITY]]) {
    expect(() => parseReport(report({ block: { pos: position } }))).toThrow(ReportError);
  }
});

test("a non-finite motion vector is refused rather than drawn", () => {
  expect(() =>
    parseReport(
      report({
        block: { prediction: { kind: "inter", reference: "last", mv_q4: [0, Number.NaN] } },
      }),
    ),
  ).toThrow(ReportError);
});

test("a non-finite mismatch offset is refused, and null is not", () => {
  expect(() => parseReport(report({ frame: { first_mismatch_offset: Number.NaN } }))).toThrow(
    ReportError,
  );
  expect(parseReport(report({ frame: { first_mismatch_offset: null } })).firstMismatchOffset).toBeNull();
  expect(parseReport(report({ frame: { first_mismatch_offset: 7 } })).firstMismatchOffset).toBe(7);
});

test("a report from a future probe version is refused by version, not by field", () => {
  expect(() => parseReport({ ...(report() as object), probe_version: 2 })).toThrow(
    /probe version 2/,
  );
});

test("overlay initials are distinct, so a link cannot turn on an overlay nobody asked for", () => {
  // The link encoding writes one initial per overlay that is on. That is only
  // reversible while the initials are distinct: a seventh overlay named
  // `prediction` or `motion vectors` would share a letter with one that
  // exists, and every link with the older one on would silently arrive with
  // both. Nothing else in this repository would notice.
  const initials = OVERLAY_NAMES.map((name) => name[0]);
  expect(new Set(initials).size).toBe(OVERLAY_NAMES.length);
});

test("every view state survives the round trip through a link", () => {
  // Exhaustive over the overlay combinations, because that is the field the
  // encoding compresses and the only one where a collision could hide.
  const states: ViewState[] = [];
  for (let mask = 0; mask < 1 << OVERLAY_NAMES.length; mask += 1) {
    const overlays = { ...DEFAULT_VIEW.overlays };
    OVERLAY_NAMES.forEach((name, index) => {
      overlays[name] = (mask & (1 << index)) !== 0;
    });
    states.push({ ...DEFAULT_VIEW, overlays });
  }
  for (const tab of ["projection", "cutting", "curves"] as const) {
    states.push({ ...DEFAULT_VIEW, tab });
  }
  states.push({ ...DEFAULT_VIEW, frame: 419, basis: "emitted", selection: [128, 64] });

  for (const state of states) {
    expect(decodeView(`#${encodeView(state)}`)).toEqual(state);
  }
});

test("a link a stranger typed falls back to the default rather than to NaN", () => {
  // Every one of these would reach a canvas coordinate or an array index if it
  // were taken at face value.
  for (const fragment of [
    "#f=-1",
    "#f=NaN",
    "#f=1e9999",
    "#f=",
    "#t=elsewhere",
    "#h=guessed",
    "#b=4",
    "#b=-1,0",
    "#b=x,y",
    "#b=,",
    "#",
    "",
  ]) {
    const view = decodeView(fragment);
    expect(Number.isInteger(view.frame)).toBe(true);
    expect(view.frame).toBeGreaterThanOrEqual(0);
    if (view.selection) {
      expect(Number.isInteger(view.selection[0])).toBe(true);
      expect(Number.isInteger(view.selection[1])).toBe(true);
    }
    expect(["projection", "cutting", "curves"]).toContain(view.tab);
    expect(["modeled", "emitted"]).toContain(view.basis);
  }
});

test("a catalogue entry missing a field is refused rather than rendered blank", () => {
  const entry = {
    id: "000",
    crate: "kf-ref",
    foundBy: "fuzz",
    fixedIn: "v0.11.0",
    regression: "conformance/crashes/false_sync_prefix.kfv",
    stream: "./regressions/false_sync_prefix.kfv",
    title: "A false sync prefix split the two decoders",
    body: "Symptom, hunt, root cause, fix, lesson.",
  };
  expect(parseCatalogue({ entries: [entry] })).toEqual([entry]);

  // Every field but the body: a finding with an empty story is thin, a finding
  // with no pinned stream is one nobody can reproduce.
  for (const missing of ["id", "crate", "foundBy", "fixedIn", "regression", "stream", "title"]) {
    expect(() => parseCatalogue({ entries: [{ ...entry, [missing]: "" }] })).toThrow(
      new RegExp(missing),
    );
    const withoutField: Record<string, unknown> = { ...entry };
    delete withoutField[missing];
    expect(() => parseCatalogue({ entries: [withoutField] })).toThrow(new RegExp(missing));
  }
  expect(() => parseCatalogue({})).toThrow(/entries array/);
  expect(() => parseCatalogue(null)).toThrow(/entries array/);
  expect(parseCatalogue({ entries: [] })).toEqual([]);
});

test("the page's Y4M reader accepts exactly what the codec's own reader accepts", () => {
  // Two readers of one format. The Rust one takes JPEG-sited 4:2:0 at even
  // dimensions within the declared picture bounds and nothing else; this one
  // used to take three more chroma sitings and any positive dimensions, so the
  // page would load a clip `kfenc` could not have produced the stream from and
  // draw an error map against it.
  //
  // The cases below are derived from `SOURCE_LIMITS` rather than written out.
  // Spelling the bounds here would have made this suite a third copy of them,
  // and the copy that agrees with the reader whatever the specification says.
  const clip = (header: string, frames = 1) => {
    const parts = [new TextEncoder().encode(`${header}\n`)];
    for (let index = 0; index < frames; index += 1) {
      parts.push(new TextEncoder().encode("FRAME\n"));
      parts.push(new Uint8Array((64 * 64 * 3) / 2));
    }
    const total = parts.reduce((sum, part) => sum + part.length, 0);
    const bytes = new Uint8Array(total);
    let at = 0;
    for (const part of parts) {
      bytes.set(part, at);
      at += part.length;
    }
    return bytes;
  };

  const { minWidth, minHeight, maxWidth, maxHeight, chroma: siting } = SOURCE_LIMITS;
  const smallest = `W${minWidth} H${minHeight}`;

  // Accepted: the one siting, named or omitted.
  expect(parseY4m(clip(`YUV4MPEG2 ${smallest} F24:1 Ip C${siting}`)).width).toBe(minWidth);
  expect(parseY4m(clip(`YUV4MPEG2 ${smallest} F24:1 Ip`)).width).toBe(minWidth);

  // Refused: the sitings the codec does not code.
  for (const chroma of ["420", "420mpeg2", "420paldv", "422", "444"]) {
    expect(chroma).not.toBe(siting);
    expect(() => parseY4m(clip(`YUV4MPEG2 ${smallest} F24:1 Ip C${chroma}`))).toThrow(SourceError);
  }

  // Refused: one step outside each declared bound, and an odd dimension.
  for (const header of [
    `YUV4MPEG2 W${minWidth - 2} H${minHeight} F24:1 Ip C${siting}`,
    `YUV4MPEG2 W${minWidth} H${minHeight - 2} F24:1 Ip C${siting}`,
    `YUV4MPEG2 W${minWidth + 1} H${minHeight} F24:1 Ip C${siting}`,
    `YUV4MPEG2 W${maxWidth + 2} H${minHeight} F24:1 Ip C${siting}`,
    `YUV4MPEG2 W${minWidth} H${maxHeight + 2} F24:1 Ip C${siting}`,
  ]) {
    expect(() => parseY4m(clip(header))).toThrow(/dimensions this format allows/);
  }
});
