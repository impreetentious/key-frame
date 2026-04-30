#!/usr/bin/env node

// Turns `docs/cutting-room/` into something the projection room can serve.
//
// The catalogue's source of truth is the markdown, because that is what a
// reader clones and what review happens against. This script reads it, checks
// that every entry declares the fields the catalogue promises, and writes one
// JSON file plus a copy of each pinned regression stream, so the page can list
// findings and open the exact stream that produced one.
//
// It refuses rather than skips. An entry missing its regression path would
// otherwise appear in the site as a finding nobody can reproduce, which is the
// one thing a bug catalogue must never contain.

import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "docs/cutting-room");
const publicDir = path.join(root, "inspector/public");
const regressionDir = path.join(publicDir, "regressions");

const REQUIRED = ["id", "crate", "found-by", "regression", "fixed-in"];
const FOUND_BY = new Set(["fuzz", "differential", "property", "conformance"]);

function parseFrontmatter(text, file) {
  if (!text.startsWith("---\n")) {
    throw new Error(`${file}: no frontmatter block`);
  }
  const end = text.indexOf("\n---\n", 4);
  if (end === -1) throw new Error(`${file}: unterminated frontmatter block`);
  const fields = {};
  for (const line of text.slice(4, end).split("\n")) {
    if (!line.trim()) continue;
    const separator = line.indexOf(":");
    if (separator === -1) throw new Error(`${file}: frontmatter line "${line}" has no key`);
    const key = line.slice(0, separator).trim();
    const value = line
      .slice(separator + 1)
      .trim()
      .replace(/^"(.*)"$/, "$1");
    fields[key] = value;
  }
  for (const key of REQUIRED) {
    if (!fields[key]) throw new Error(`${file}: frontmatter is missing ${key}`);
  }
  if (!FOUND_BY.has(fields["found-by"])) {
    throw new Error(`${file}: found-by "${fields["found-by"]}" is not one of ${[...FOUND_BY].join(", ")}`);
  }
  return { fields, body: text.slice(end + 5) };
}

function main() {
  const files = readdirSync(source)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .sort();

  mkdirSync(publicDir, { recursive: true });
  mkdirSync(regressionDir, { recursive: true });

  const entries = files.map((name) => {
    const text = readFileSync(path.join(source, name), "utf8");
    const { fields, body } = parseFrontmatter(text, name);

    const regression = path.join(root, fields["regression"]);
    if (!existsSync(regression)) {
      throw new Error(`${name}: the regression stream ${fields["regression"]} does not exist`);
    }
    const streamName = path.basename(regression);
    copyFileSync(regression, path.join(regressionDir, streamName));

    // The first heading is the finding's title; the rest is its story.
    const titleMatch = body.match(/^#\s+(.+)$/m);
    return {
      id: fields["id"],
      crate: fields["crate"],
      foundBy: fields["found-by"],
      fixedIn: fields["fixed-in"],
      regression: fields["regression"],
      stream: `./regressions/${streamName}`,
      title: titleMatch?.[1]?.trim() ?? name.replace(/\.md$/, ""),
      body: body.replace(/^#\s+.+$\n?/m, "").trim(),
    };
  });

  writeFileSync(
    path.join(publicDir, "cutting-room.json"),
    `${JSON.stringify({ version: 1, entries }, null, 2)}\n`,
  );
  console.log(
    `cutting-room: OK — ${entries.length} entr${entries.length === 1 ? "y" : "ies"} with pinned regression streams`,
  );
}

try {
  main();
} catch (error) {
  console.error(`cutting-room: FAILED — ${error instanceof Error ? error.message : error}`);
  process.exit(1);
}
