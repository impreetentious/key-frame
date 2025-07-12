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

import { existsSync, readFileSync } from "node:fs";
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

// A published rate-distortion number is only reproducible if the encoder build
// that produced it can be named. A hand-maintained version string would drift.
for (const file of ["crates/kf-tools/src/main.rs"]) {
  if (!has(file)) continue;
  if (!read(file).includes('env!("CARGO_PKG_VERSION")')) {
    errors.push(`${file} must derive runtime versions from CARGO_PKG_VERSION`);
  }
}

if (errors.length) {
  console.error("version-coherence FAILED:");
  for (const error of errors) console.error(` - ${error}`);
  process.exit(1);
}

console.log(`version-coherence OK — ${version}`);
