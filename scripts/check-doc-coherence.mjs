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
});

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
