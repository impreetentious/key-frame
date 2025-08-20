#!/usr/bin/env node

// Native and WebAssembly must decode to the same bytes, or the projection room
// is showing something the conformance suite never proved.
//
// The module is loaded exactly as a browser loads it: `WebAssembly.instantiate`
// with no import object, because the module imports nothing. There is no
// binding generator in the path, so the artifact checked here is the artifact
// the application ships — a gate that ran a differently built module would
// prove nothing about the one people load.
//
// Every committed conformance stream is decoded twice inside the module: once
// as a whole stream and once frame by frame through random access. Both are
// compared against the decoded-YUV hash the native decoders committed. A wasm
// build that agreed with itself but not with the manifest would otherwise look
// perfectly healthy.
//
// With `--probe-out DIR` the module's syntax JSON for each stream is written
// there, so the gate script can diff it against what the native `kfprobe`
// writes. The inspector reads that JSON for every overlay it draws, and a
// number that differed between the two builds would be a wrong picture rather
// than a wrong pixel — harder to notice and no less wrong.

import { createHash } from "node:crypto";
import { gzipSync } from "node:zlib";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const modulePath = path.join(
  root,
  "target/wasm32-unknown-unknown/release/kf_wasm.wasm",
);

// Appendix-level budget for what a browser downloads, checked on the compressed
// artifact because that is what crosses the network.
const WASM_GZIP_BUDGET_BYTES = 1.5 * 1024 * 1024;
const EXPECTED_ABI_VERSION = 1;

const failures = [];

const probeFlag = process.argv.indexOf("--probe-out");
const probeOutDir = probeFlag === -1 ? null : process.argv[probeFlag + 1];
if (probeFlag !== -1 && !probeOutDir) {
  console.error("wasm-equality: --probe-out needs a directory");
  process.exit(1);
}
if (probeOutDir) mkdirSync(probeOutDir, { recursive: true });

function parseManifest(text) {
  const vectors = [];
  let current = null;
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed === "[[vectors]]") {
      current = {};
      vectors.push(current);
      continue;
    }
    if (!current) continue;
    const stringMatch = trimmed.match(/^([a-z_0-9]+)\s*=\s*"([^"]*)"$/);
    if (stringMatch) {
      current[stringMatch[1]] = stringMatch[2];
      continue;
    }
    const numberMatch = trimmed.match(/^([a-z_0-9]+)\s*=\s*(-?\d+)$/);
    if (numberMatch) current[numberMatch[1]] = Number(numberMatch[2]);
  }
  return vectors;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

/// A fresh instance per stream, so no vector can pass because a previous one
/// left state behind.
async function instantiate(moduleBytes) {
  const { instance } = await WebAssembly.instantiate(moduleBytes, {});
  return instance.exports;
}

function writeStream(exports, bytes) {
  const offset = exports.kf_alloc(bytes.length);
  if (offset === 0) throw new Error("the module refused to allocate");
  new Uint8Array(exports.memory.buffer, offset, bytes.length).set(bytes);
  return offset;
}

function readOutput(exports) {
  const offset = exports.kf_output_ptr();
  const length = exports.kf_output_len();
  // Read the view after the call, never before: any allocation inside the
  // module may have grown the memory and detached an earlier view.
  return Buffer.from(
    new Uint8Array(exports.memory.buffer, offset, length).slice(),
  );
}

function readMessage(exports) {
  const offset = exports.kf_message_ptr();
  const length = exports.kf_message_len();
  if (length === 0) return "";
  return Buffer.from(
    new Uint8Array(exports.memory.buffer, offset, length).slice(),
  ).toString("utf8");
}

async function checkVector(moduleBytes, vector) {
  const streamPath = path.join(root, "conformance", vector.stream);
  const bytes = readFileSync(streamPath);
  const label = `${vector.origin}:${vector.name}`;

  const exports = await instantiate(moduleBytes);
  if (exports.kf_abi_version() !== EXPECTED_ABI_VERSION) {
    failures.push(
      `${label}: the module reports boundary version ${exports.kf_abi_version()}, expected ${EXPECTED_ABI_VERSION}`,
    );
    return;
  }

  const offset = writeStream(exports, bytes);
  const openStatus = exports.kf_open(offset, bytes.length);
  exports.kf_free(offset, bytes.length);
  if (openStatus !== 0) {
    failures.push(`${label}: open failed (${openStatus}): ${readMessage(exports)}`);
    return;
  }

  if (exports.kf_width() !== vector.width || exports.kf_height() !== vector.height) {
    failures.push(
      `${label}: the module reports ${exports.kf_width()}x${exports.kf_height()}, the manifest says ${vector.width}x${vector.height}`,
    );
  }
  if (exports.kf_frame_count() !== vector.frame_count) {
    failures.push(
      `${label}: the module counts ${exports.kf_frame_count()} frames, the manifest says ${vector.frame_count}`,
    );
  }

  const wholeStatus = exports.kf_decode_all();
  if (wholeStatus !== 0) {
    failures.push(
      `${label}: whole-stream decode failed (${wholeStatus}): ${readMessage(exports)}`,
    );
    return;
  }
  const whole = readOutput(exports);
  const wholeHash = sha256(whole);
  if (wholeHash !== vector.decoded_yuv_sha256) {
    failures.push(
      `${label}: WebAssembly decoded to ${wholeHash}, the native decoders committed ${vector.decoded_yuv_sha256}`,
    );
  }

  const perFrame = [];
  for (let index = 0; index < vector.frame_count; index += 1) {
    const status = exports.kf_decode_frame(index);
    if (status !== 0) {
      failures.push(
        `${label}: seek to frame ${index} failed (${status}): ${readMessage(exports)}`,
      );
      return;
    }
    const entry = exports.kf_last_entry_keyframe();
    if (entry > index) {
      failures.push(
        `${label}: seek to frame ${index} claims to have restarted at ${entry}`,
      );
    }
    perFrame.push(readOutput(exports));
  }
  const stitched = Buffer.concat(perFrame);
  if (!stitched.equals(whole)) {
    failures.push(
      `${label}: decoding frame by frame does not reproduce the whole-stream decode`,
    );
  }

  if (exports.kf_decode_frame(vector.frame_count) === 0) {
    failures.push(`${label}: seeking past the last frame succeeded`);
  }
  if (exports.kf_probe_frame(vector.frame_count) === 0) {
    failures.push(`${label}: probing past the last frame succeeded`);
  }

  const probeStatus = exports.kf_probe_frame(0);
  if (probeStatus !== 0) {
    failures.push(`${label}: probe failed (${probeStatus}): ${readMessage(exports)}`);
    return;
  }
  const probeJson = readOutput(exports);
  if (probeJson.length === 0) {
    failures.push(`${label}: the probe produced no output`);
    return;
  }
  try {
    JSON.parse(probeJson.toString("utf8"));
  } catch (error) {
    failures.push(`${label}: the probe output is not valid JSON: ${error.message}`);
    return;
  }
  if (probeOutDir) {
    // `kfprobe` prints its report as a line, so the written file carries the
    // same trailing newline. The comparison is about the report, not about
    // which side happens to terminate the file.
    writeFileSync(
      path.join(probeOutDir, `${vector.origin}_${vector.name}.json`),
      Buffer.concat([probeJson, Buffer.from("\n")]),
    );
  }
}

async function main() {
  let moduleBytes;
  try {
    moduleBytes = readFileSync(modulePath);
  } catch {
    console.error(
      `wasm-equality: FAILED — ${path.relative(root, modulePath)} is missing; build it with the wasm gate rather than running this directly`,
    );
    process.exit(1);
  }

  const compressed = gzipSync(moduleBytes, { level: 9 }).length;
  if (compressed > WASM_GZIP_BUDGET_BYTES) {
    failures.push(
      `the module is ${compressed} bytes gzipped, over the ${WASM_GZIP_BUDGET_BYTES} byte budget`,
    );
  }

  const manifest = readFileSync(
    path.join(root, "conformance/manifest.toml"),
    "utf8",
  );
  const vectors = parseManifest(manifest);
  if (vectors.length === 0) {
    failures.push("the conformance manifest lists no vectors, so this gate would be vacuous");
  }
  for (const vector of vectors) {
    await checkVector(moduleBytes, vector);
  }

  if (failures.length > 0) {
    console.error("wasm-equality: FAILED");
    for (const failure of failures) console.error(` - ${failure}`);
    process.exit(1);
  }

  const frames = vectors.reduce((total, vector) => total + vector.frame_count, 0);
  console.log(
    `wasm-equality: OK — ${vectors.length} stream(s), ${frames} frame(s), whole-stream and frame-by-frame decodes match the native hashes; module is ${compressed} bytes gzipped`,
  );
}

await main();
