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

const adrFiles = readdirSync(path.join(root, "docs/adr"))
  .filter((name) => /^\d{4}-.*\.md$/.test(name))
  .sort();
adrFiles.forEach((name, expected) => {
  const actual = Number.parseInt(name.slice(0, 4), 10);
  if (actual !== expected) {
    errors.push(`ADR sequence expected ${String(expected).padStart(4, "0")}, found ${name}`);
  }
  // And the title, which this used never to look at. Three records opened with
  // `ADR NNNN` where the template and every cross-reference in the repository
  // write `ADR-NNNN`, so a search for the canonical form missed them.
  if (name === "0000-template.md") return;
  const title = readFileSync(path.join(root, "docs/adr", name), "utf8").split("\n")[0] ?? "";
  const expectedTitle = new RegExp(`^# ADR-${name.slice(0, 4)}: \\S`);
  if (!expectedTitle.test(title)) {
    errors.push(`${name} should open with "# ADR-${name.slice(0, 4)}: …", found ${JSON.stringify(title)}`);
  }
});

// The index has to name every record, or it is a map with roads missing.
const indexPath = path.join(root, "docs/adr/README.md");
if (!existsSync(indexPath)) {
  errors.push("docs/adr/README.md is missing, so the decision records have no index");
} else {
  const index = readFileSync(indexPath, "utf8");
  for (const name of adrFiles) {
    if (name === "0000-template.md") continue;
    if (!index.includes(name)) errors.push(`docs/adr/README.md does not name ${name}`);
  }
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
  ["cargo fmt", "formatting"],
  ["cargo clippy", "lints"],
  ["forbidden API scan and decoder boundary", "forbidden-API"],
  ["tests", "the full test suite"],
  ["rustdoc", "the documentation build"],
]);

const verifySection = readme.split("\n## Verify\n")[1]?.split("\n## ")[0] ?? "";
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
