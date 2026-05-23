#!/usr/bin/env node

// One version, every surface. A release that ships a README claiming one
// version, a workspace manifest claiming another, and a lockfile claiming a
// third is not a release; it is three of them wearing a trench coat. Each
// check below is guarded by `existsSync`, so the script is correct in a partial
// checkout and so a surface that is absent is never silently assumed to agree.
//
// The bitstream version is deliberately not checked here. It is a separate
// contract with its own lifetime: a repository release may happen without a
// syntax change, and a syntax change must be an explicit decision rather than
// a side effect of bumping a patch number.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (file) => readFileSync(path.join(root, file), "utf8");
const has = (file) => existsSync(path.join(root, file));
const errors = [];
const semver = "([0-9]+\\.[0-9]+\\.[0-9]+)";

const readmeVersion = read("README.md").match(
  new RegExp(`\\*\\*Version:\\*\\*\\s+v${semver}(?:\\s|$)`),
)?.[1];
if (!readmeVersion) errors.push("README.md missing **Version:** vX.Y.Z marker");

let version = readmeVersion;
if (has("Cargo.toml")) {
  const cargoVersion = read("Cargo.toml").match(
    new RegExp(`\\[workspace\\.package\\][\\s\\S]*?^version\\s*=\\s*"${semver}"`, "m"),
  )?.[1];
  if (!cargoVersion) errors.push("Cargo.toml missing workspace package version");
  if (version && cargoVersion !== version) {
    errors.push(`Cargo.toml version ${cargoVersion ?? "missing"} != README.md ${version}`);
  }
  version = cargoVersion ?? version;
}

if (version && has("Cargo.lock")) {
  for (const block of read("Cargo.lock").split("[[package]]").slice(1)) {
    const name = block.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1];
    const packageVersion = block.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
    if (name?.startsWith("kf-") && packageVersion !== version) {
      errors.push(`Cargo.lock ${name} version ${packageVersion ?? "missing"} != ${version}`);
    }
  }
}

if (has("rust-toolchain.toml")) {
  const channel = read("rust-toolchain.toml").match(/^channel\s*=\s*"([^"]+)"/m)?.[1];
  if (!channel || !/^\d+\.\d+\.\d+$/.test(channel)) {
    errors.push(`rust-toolchain.toml channel ${channel ?? "missing"} is not an exact version`);
  }
  if (has(".github/workflows/check.yml")) {
    const workflow = read(".github/workflows/check.yml");
    if (/toolchain:\s*(stable|\d)/.test(workflow)) {
      errors.push("check workflow duplicates the Rust version instead of reading rust-toolchain.toml");
    }
  }
}

if (version && has("inspector/package.json")) {
  const pkg = JSON.parse(read("inspector/package.json"));
  if (pkg.version !== version) {
    errors.push(`inspector/package.json version ${pkg.version} != ${version}`);
  }
  if (has("inspector/package-lock.json")) {
    const lock = JSON.parse(read("inspector/package-lock.json"));
    if (lock.version !== version) {
      errors.push(`inspector/package-lock.json version ${lock.version} != ${version}`);
    }
    if (lock.packages?.[""]?.version !== version) {
      errors.push(`inspector lock root version ${lock.packages?.[""]?.version} != ${version}`);
    }
  }
}

// A published figure is only reproducible if the build that produced it can be
// named, so every receipt this repository writes carries an `encoder_version`
// and every one of those has to come from the compiler rather than from a hand.
//
// This used to name one file — `crates/kf-tools/src/main.rs` — guarded by an
// existence check, and that file has never existed in this layout: the tools are
// separate binaries under `src/bin/`. So the check skipped, silently, for its
// whole life, over a rule the receipts depend on.
//
// It finds the emitters now instead of naming them. A source that writes an
// `encoder_version` field must derive it from `CARGO_PKG_VERSION`, and may not
// carry a version-shaped literal of its own — which is the shape the drift
// would actually take.
const emitters = [];
for (const directory of ["crates"]) {
  const walk = (at) => {
    for (const entry of readdirSync(path.join(root, at), { withFileTypes: true })) {
      if (entry.name === "target") continue;
      const next = `${at}/${entry.name}`;
      if (entry.isDirectory()) walk(next);
      else if (entry.name.endsWith(".rs")) emitters.push(next);
    }
  };
  walk(directory);
}

let checked = 0;
for (const file of emitters) {
  const body = read(file);
  if (!body.includes('"encoder_version"')) continue;
  checked += 1;
  if (!body.includes('env!("CARGO_PKG_VERSION")')) {
    errors.push(`${file} writes an encoder_version that does not come from CARGO_PKG_VERSION`);
  }
  const literal = body.match(/string\("(\d+\.\d+\.\d+)"\)/);
  if (literal) {
    errors.push(`${file} carries the version literal ${literal[1]}, which will not follow a bump`);
  }
}
if (checked === 0) {
  errors.push("no source writes an encoder_version, so the receipt version check is vacuous");
}

if (errors.length) {
  console.error("version-coherence FAILED:");
  for (const error of errors) console.error(` - ${error}`);
  process.exit(1);
}

console.log(`version-coherence OK — ${version}, ${checked} receipt version(s) from the compiler`);
