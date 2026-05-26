#!/usr/bin/env node

// The closed sets the projection room states, against the ones that define them.
//
// The page carries three tables of names it did not invent: a glyph per intra
// mode, a colour per encoder toolset, and the overlay list its shared links
// encode. Each is a copy of a set that lives somewhere else — in a frozen
// specification asset, or in the encoder — and each has a quiet fallback for a
// name it does not recognise. A mode with no glyph was drawn as DC, which is a
// confident picture of a flat average for a prediction that is not one; a
// toolset with no colour is drawn in the ink colour, indistinguishable from
// the axis it sits on.
//
// Neither would fail anything. The page would render, the tests would pass, and
// the chart would be wrong in a way that looks like a chart. So the sets are
// compared here, where both sides can be read.
//
// This is not typechecked by the page's TypeScript and does not need Node types
// on that side, which is why it lives in `scripts/` rather than in the browser
// test suite: the inspector deliberately carries no `@types/node`.

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const errors = [];

/// The names inside a `["a", "b"]` list on one line of a TOML asset.
function declaredList(file, key) {
  const match = read(file).match(new RegExp(`^${key} = \\[(.*)\\]$`, "m"));
  if (!match) throw new Error(`${file} declares no ${key}`);
  return match[1]
    .split(",")
    .map((name) => name.trim().replace(/^"|"$/g, ""))
    .filter((name) => name.length > 0);
}

/// The keys of a `const NAME: Record<string, …> = { … }` object literal.
function tableKeys(file, name) {
  const body = read(file);
  const start = body.indexOf(`const ${name}`);
  if (start === -1) throw new Error(`${file} declares no ${name}`);
  const open = body.indexOf("{", start);
  const close = body.indexOf("\n};", open);
  if (open === -1 || close === -1) throw new Error(`${name} in ${file} is not an object literal`);
  return [...body.slice(open, close).matchAll(/^\s*"?([A-Za-z][A-Za-z0-9-]*)"?\s*:/gm)].map(
    (match) => match[1],
  );
}

/// One declared scalar or quoted string from a frozen asset.
function declaredValue(file, key) {
  const match = read(file).match(new RegExp(`^${key} = "?([^"\n]+?)"?\\s*$`, "m"));
  if (!match) throw new Error(`${file} declares no ${key}`);
  return match[1];
}

/// The value of one property of a `const NAME = { … } as const` object literal.
function tableValue(file, name, property) {
  const body = read(file);
  const start = body.indexOf(`const ${name}`);
  if (start === -1) throw new Error(`${file} declares no ${name}`);
  const open = body.indexOf("{", start);
  const close = body.indexOf("\n}", open);
  if (open === -1 || close === -1) throw new Error(`${name} in ${file} is not an object literal`);
  const match = body
    .slice(open, close)
    .match(new RegExp(`^\\s*${property}:\\s*"?([^",\n]+)"?,`, "m"));
  if (!match) throw new Error(`${name} in ${file} has no ${property}`);
  return match[1].trim();
}

function compareValue(label, mine, theirs) {
  if (mine !== theirs) {
    errors.push(`${label}: the page says ${JSON.stringify(mine)}, the specification says ${JSON.stringify(theirs)}`);
  }
}

function compare(label, mine, theirs) {
  const missing = theirs.filter((name) => !mine.includes(name));
  const extra = mine.filter((name) => !theirs.includes(name));
  if (missing.length > 0) {
    errors.push(`${label}: the page has nothing for ${missing.join(", ")}`);
  }
  if (extra.length > 0) {
    errors.push(`${label}: the page names ${extra.join(", ")}, which nothing declares`);
  }
  if (theirs.length === 0) {
    errors.push(`${label}: nothing was found to compare against, so this checked nothing`);
  }
}

// Intra modes: six drawn at an angle, and two drawn without one because they
// have no direction.
compare(
  "intra glyphs",
  ["dc", "planar", ...tableKeys("inspector/src/render.ts", "INTRA_ANGLES")],
  declaredList("spec/v1/intra.toml", "modes"),
);

// Encoder toolsets: the closed list the receipts name and the charts colour.
const toolsets = [
  ...read("crates/kf-enc/src/toolset.rs")
    .match(/pub const fn names\(\) -> \[&'static str; \d+\] \{\s*\[([^\]]*)\]/)?.[1]
    .matchAll(/"([^"]+)"/g),
].map((match) => match[1]);
compare("toolset colours", tableKeys("inspector/src/Curves.tsx", "TOOLSET_COLOURS"), toolsets);
compare("toolset notes", tableKeys("inspector/src/Curves.tsx", "TOOLSET_NOTES"), toolsets);

// The page's Y4M reader, against the picture bounds and chroma siting the codec
// declares.
//
// The reader had these as five literals, with a comment saying they were "the
// same bounds the codec's own reader enforces" — true, and true because two
// people typed the same numbers. A raised `max_width` would have left `kfenc`
// encoding a clip the page then refused to load as its source, which is the
// one comparison the page exists to make. The page ships as a static bundle
// and cannot read a TOML asset at runtime, so the reconciliation belongs here.
const constants = "spec/v1/constants.toml";
for (const [property, key] of [
  ["minWidth", "min_width"],
  ["maxWidth", "max_width"],
  ["minHeight", "min_height"],
  ["maxHeight", "max_height"],
]) {
  compareValue(
    `source reader ${key}`,
    tableValue("inspector/src/source.ts", "SOURCE_LIMITS", property),
    declaredValue(constants, key),
  );
}

// The Y4M token carries the format's leading `C`; a chroma siting named on its
// own does not, the same way the probe schema states it.
compareValue(
  "source reader chroma siting",
  tableValue("inspector/src/source.ts", "SOURCE_LIMITS", "chroma"),
  declaredValue(constants, "chroma_name").replace(/^C/, ""),
);

// The quantizer ceiling the QP overlay normalises by.
compareValue(
  "quantizer tint ceiling",
  read("inspector/src/render.ts").match(/^export const QUANTIZER_CEILING = (\d+);$/m)?.[1],
  declaredValue(constants, "qp_max"),
);

if (errors.length > 0) {
  console.error("interface-coherence: the page and the specification name different sets");
  for (const error of errors) console.error(` - ${error}`);
  console.error("  A name the page does not know is drawn with a fallback that looks deliberate.");
  process.exit(1);
}

console.log("interface-coherence: OK — the page's closed sets and declared bounds are the specification's");
