#!/usr/bin/env node

// The projection room's size budget, checked on what a browser downloads.
//
// Compressed bytes, because that is what crosses the network, and separate
// budgets for script and decoder, because they are different problems: script
// grows when the page gains features, and the module grows when the codec
// does. One combined number would let either hide inside the other.
//
// A budget nobody can fail is decoration, so this also refuses when there is
// nothing to measure: a missing build is a failure, not a pass.

import { gzipSync } from "node:zlib";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const dist = path.join(root, "inspector/dist");

// The budgets are declared in the frozen constants and read from there rather
// than written twice. A number that lives in two files is a number that will
// eventually disagree with itself, and the generated documentation publishes
// these — so a budget quietly raised here would leave the document promising a
// smaller page than the gate enforces.
function declaredConstant(name) {
  const source = readFileSync(path.join(root, "spec/v1/constants.toml"), "utf8");
  const match = source.match(new RegExp(`^${name}\\s*=\\s*(\\d+)\\s*$`, "m"));
  if (!match) throw new Error(`spec/v1/constants.toml declares no ${name}`);
  return Number.parseInt(match[1], 10);
}

const SCRIPT_AND_STYLE_BUDGET = declaredConstant("inspector_js_css_gzip_bytes");
const MODULE_BUDGET = declaredConstant("inspector_wasm_gzip_bytes");

const failures = [];

function gzippedSize(file) {
  return gzipSync(readFileSync(file), { level: 9 }).length;
}

function walk(directory) {
  const found = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...walk(full));
    else found.push(full);
  }
  return found;
}

if (!existsSync(dist)) {
  console.error("site-budget: FAILED — inspector/dist is missing; build the site first");
  process.exit(1);
}

const files = walk(dist);
const scripts = files.filter((file) => file.endsWith(".js") || file.endsWith(".css"));
const modules = files.filter((file) => file.endsWith(".wasm"));

if (scripts.length === 0) {
  failures.push("the build produced no script or stylesheet, so this budget would be vacuous");
}
if (modules.length === 0) {
  failures.push("the build produced no WebAssembly module; the page would have no decoder");
}

const scriptBytes = scripts.reduce((total, file) => total + gzippedSize(file), 0);
const moduleBytes = modules.reduce((total, file) => total + gzippedSize(file), 0);

if (scriptBytes > SCRIPT_AND_STYLE_BUDGET) {
  failures.push(
    `script and stylesheet are ${scriptBytes} bytes gzipped, over the ${SCRIPT_AND_STYLE_BUDGET} byte budget`,
  );
}
if (moduleBytes > MODULE_BUDGET) {
  failures.push(
    `the decoder module is ${moduleBytes} bytes gzipped, over the ${MODULE_BUDGET} byte budget`,
  );
}

// Nothing decoded may be baked into the page. The site decodes live or it is
// not the site this repository claims to ship.
for (const file of files) {
  if (file.endsWith(".yuv") || file.endsWith(".y4m")) {
    failures.push(`${path.relative(root, file)} is decoded video shipped as a fixture`);
  }
}

if (failures.length > 0) {
  console.error("site-budget: FAILED");
  for (const failure of failures) console.error(` - ${failure}`);
  process.exit(1);
}

console.log(
  `site-budget: OK — ${scriptBytes} bytes of script and style, ${moduleBytes} bytes of decoder, both gzipped and inside budget`,
);
