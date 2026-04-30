// A very small markdown renderer, for the cutting room's entries only.
//
// It handles exactly what those entries use: headings, paragraphs, bullet
// lists, fenced code, inline code, and emphasis. A library would be a hundred
// times this size and would bring a sanitizer question with it; this returns a
// tree of plain values that React renders as elements, so nothing is ever
// injected as HTML.
//
// Anything it does not recognise stays as text. That is deliberate: a
// catalogue entry showing a stray asterisk is a formatting nuisance, while an
// entry silently swallowing a line is a lost finding.

export type Inline = { kind: "text"; text: string } | { kind: "code"; text: string } | {
  kind: "strong";
  text: string;
};

export type Block =
  | { kind: "heading"; level: 2 | 3; inlines: Inline[] }
  | { kind: "paragraph"; inlines: Inline[] }
  | { kind: "list"; items: Inline[][] }
  | { kind: "code"; text: string };

/// Splits a line into text, inline code, and bold runs.
///
/// Backticks win over asterisks: a code span is literal by definition, so
/// emphasis markers inside one are part of the code.
export function parseInlines(line: string): Inline[] {
  const inlines: Inline[] = [];
  let rest = line;
  while (rest.length > 0) {
    const code = rest.match(/`([^`]+)`/);
    const strong = rest.match(/\*\*([^*]+)\*\*/);
    const codeAt = code?.index ?? Infinity;
    const strongAt = strong?.index ?? Infinity;
    if (codeAt === Infinity && strongAt === Infinity) {
      inlines.push({ kind: "text", text: rest });
      break;
    }
    if (codeAt <= strongAt && code) {
      if (codeAt > 0) inlines.push({ kind: "text", text: rest.slice(0, codeAt) });
      inlines.push({ kind: "code", text: code[1] ?? "" });
      rest = rest.slice(codeAt + (code[0]?.length ?? 0));
    } else if (strong) {
      if (strongAt > 0) inlines.push({ kind: "text", text: rest.slice(0, strongAt) });
      inlines.push({ kind: "strong", text: strong[1] ?? "" });
      rest = rest.slice(strongAt + (strong[0]?.length ?? 0));
    }
  }
  return inlines;
}

export function parseMarkdown(text: string): Block[] {
  const blocks: Block[] = [];
  const lines = text.split("\n");
  let index = 0;

  while (index < lines.length) {
    const line = lines[index] ?? "";

    if (line.trim() === "") {
      index += 1;
      continue;
    }

    if (line.startsWith("```")) {
      const collected: string[] = [];
      index += 1;
      while (index < lines.length && !(lines[index] ?? "").startsWith("```")) {
        collected.push(lines[index] ?? "");
        index += 1;
      }
      index += 1;
      blocks.push({ kind: "code", text: collected.join("\n") });
      continue;
    }

    const heading = line.match(/^(#{2,3})\s+(.*)$/);
    if (heading) {
      blocks.push({
        kind: "heading",
        level: heading[1]?.length === 2 ? 2 : 3,
        inlines: parseInlines(heading[2] ?? ""),
      });
      index += 1;
      continue;
    }

    if (/^[-*]\s+/.test(line)) {
      const items: Inline[][] = [];
      while (index < lines.length && /^[-*]\s+/.test(lines[index] ?? "")) {
        items.push(parseInlines((lines[index] ?? "").replace(/^[-*]\s+/, "")));
        index += 1;
      }
      blocks.push({ kind: "list", items });
      continue;
    }

    // A paragraph runs until a blank line, and its wrapped lines are rejoined
    // with spaces so the source can stay hard-wrapped at a readable width.
    const paragraph: string[] = [];
    while (
      index < lines.length &&
      (lines[index] ?? "").trim() !== "" &&
      !(lines[index] ?? "").startsWith("```") &&
      !/^#{2,3}\s+/.test(lines[index] ?? "") &&
      !/^[-*]\s+/.test(lines[index] ?? "")
    ) {
      paragraph.push((lines[index] ?? "").trim());
      index += 1;
    }
    blocks.push({ kind: "paragraph", inlines: parseInlines(paragraph.join(" ")) });
  }

  return blocks;
}
