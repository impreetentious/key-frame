#!/usr/bin/env node

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const errors = [];

for (const required of [
  "README.md",
  "LICENSE",
  ".gitattributes",
  ".gitignore",
  ".editorconfig",
  "docs",
  "scripts",
  ".github/workflows",
]) {
  if (!existsSync(path.join(root, required))) {
    errors.push(`missing required repository surface: ${required}`);
  }
}

const readme = readFileSync(path.join(root, "README.md"), "utf8").trimEnd();
if (!/^\*\*Version:\*\* v\d+\.\d+\.\d+$/.test(readme.split("\n").at(-1) ?? "")) {
  errors.push("README version footer must be the final line");
}

// The decision log used to be a directory of numbered files with a separate
// index; both are now one table in the specification, so the gate checks the
// same invariant in its new home: a gapless numbered sequence with every row
// present. A record cannot be dropped, duplicated, or added out of order
// without failing here.
const specPath = path.join(root, "docs/CODEC-SPEC.md");
if (!existsSync(specPath)) {
  errors.push("docs/CODEC-SPEC.md is missing, so the decision log has no home");
} else {
  const spec = readFileSync(specPath, "utf8");
  const numbers = [...spec.matchAll(/^\|\s*(\d{4})\s*\|/gm)].map((m) => Number.parseInt(m[1], 10));
  if (numbers.length === 0) {
    errors.push("docs/CODEC-SPEC.md has no decision-log rows");
  }
  numbers.forEach((actual, index) => {
    const expected = index + 1;
    if (actual !== expected) {
      errors.push(
        `decision log expected row ${String(expected).padStart(4, "0")}, found ${String(actual).padStart(4, "0")}`,
      );
    }
  });
}

// The README's `## Verify` block against the gate list it describes.
//
// Preflight grew from eighteen steps to thirty-four while this paragraph was
// updated some of the times a gate was added and not others. It had fallen
// three behind — interface coherence, the dependency closure, and the
// rate-distortion receipt all ran on every change and appeared nowhere a
// reader could see. A paragraph that lists most of what runs is worse than
// one that lists none of it, because it reads as complete.
//
// The map is deliberately fail-closed in both directions: a step it does not
// name is an error, so adding a gate to preflight forces both an entry here
// and a word in the README, and a named word that has gone missing from the
// paragraph is an error too.
const verifyWords = new Map([
  ["version coherence", "version"],
  ["documentation coherence", "documentation"],
  ["claim coherence", "claim"],
  ["interface coherence", "interface"],
  ["declared scalar use", "declared-scalar"],
  ["license coherence", "license"],
  ["dependency closure", "dependency"],
  ["specification closure", "specification"],
  ["range-coder gate", "range-coder"],
  ["transform and quantization gate", "transform"],
  ["bitstream header and packet gate", "bitstream"],
  ["syntax round-trip gate", "syntax"],
  ["independent reference decoder gate", "reference-decoder"],
  ["probe schema and canonical replay gate", "probe"],
  ["end-to-end intra codec gate", "intra"],
  ["native conformance gate", "native-conformance"],
  ["inter codec gate", "inter-codec"],
  ["entropy lockstep and transaction gate", "entropy"],
  ["deblock filter gate", "deblock"],
  ["natural corpus bit-exactness gate", "natural-corpus"],
  ["rate-control gate", "rate-control"],
  ["quality-metric gate", "quality-metric"],
  ["rate-distortion receipt gate", "rate-distortion"],
  ["decoder campaign gate", "decoder-campaign"],
  ["corruption and error matrix gate", "error-matrix"],
  ["conformance coverage gate", "conformance-coverage"],
  ["random-access seek gate", "random-access-seek"],
  ["native and WebAssembly equality gate", "WebAssembly-equality"],
  ["projection room build, budget, and browser smoke", "projection-room"],
  ["terminal demo", "terminal-demo"],
  ["cargo fmt", "formatting"],
  ["cargo clippy", "lints"],
  ["forbidden API scan and decoder boundary", "forbidden-API"],
  ["tests", "the full test suite"],
  ["rustdoc", "the documentation build"],
]);

// Whitespace-collapsed before matching, because these are phrases in wrapped
// prose: "the documentation build" is one word-sequence to a reader and three
// tokens with a newline in the middle to a substring search, and rewrapping a
// paragraph is not supposed to be able to fail this.
const verifySection = (readme.split("\n## Verify\n")[1]?.split("\n## ")[0] ?? "").replace(
  /\s+/g,
  " ",
);
if (!verifySection) {
  errors.push("README has no ## Verify section describing what preflight runs");
} else {
  const preflight = readFileSync(path.join(root, "scripts/preflight.sh"), "utf8");
  const steps = [...preflight.matchAll(/^echo "\[\d+\/\d+\] (.+)"$/gm)].map((m) => m[1]);
  if (!steps.length) errors.push("scripts/preflight.sh announces no numbered steps");
  for (const step of steps) {
    const word = verifyWords.get(step);
    if (word === undefined) {
      errors.push(`preflight runs "${step}" and README's Verify block has no word for it`);
    } else if (!verifySection.includes(word)) {
      errors.push(`README's Verify block does not name "${step}" (expected ${JSON.stringify(word)})`);
    }
  }
  for (const step of verifyWords.keys()) {
    if (!steps.includes(step)) {
      errors.push(`README's Verify block describes "${step}", which preflight no longer runs`);
    }
  }
}

function markdownFiles(directory) {
  const files = [];
  for (const entry of readdirSync(directory)) {
    if ([".git", "node_modules", "target"].includes(entry)) continue;
    const absolute = path.join(directory, entry);
    if (statSync(absolute).isDirectory()) files.push(...markdownFiles(absolute));
    else if (entry.endsWith(".md")) files.push(absolute);
  }
  return files;
}

for (const source of markdownFiles(root)) {
  const body = readFileSync(source, "utf8");
  for (const match of body.matchAll(/(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)/g)) {
    const target = match[1];
    if (/^(?:https?:|mailto:|#)/.test(target)) continue;
    const relative = target.split("#", 1)[0];
    const resolved = path.resolve(path.dirname(source), relative);
    if (!existsSync(resolved)) {
      errors.push(`broken local link in ${path.relative(root, source)}: ${target}`);
    }
  }
}

if (errors.length) {
  console.error("doc-coherence: FAILED");
  for (const error of errors) console.error(` - ${error}`);
  process.exit(1);
}

console.log("doc-coherence: OK");
