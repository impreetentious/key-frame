import { useEffect, useState } from "react";

import { type Block, type Inline, parseMarkdown } from "./markdown";

/// A finding, as `scripts/collect-cutting-room.mjs` publishes it.
export interface Finding {
  id: string;
  crate: string;
  foundBy: string;
  fixedIn: string;
  regression: string;
  stream: string;
  title: string;
  body: string;
}

const CATALOGUE_URL = "./cutting-room.json";

export function CuttingRoom({ onOpenStream }: { onOpenStream: (url: string, label: string) => void }) {
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const response = await fetch(CATALOGUE_URL);
        if (!response.ok) throw new Error(`the catalogue did not load (${response.status})`);
        const data: unknown = await response.json();
        if (cancelled) return;
        const entries = (data as { entries?: Finding[] }).entries;
        if (!Array.isArray(entries)) throw new Error("the catalogue has no entries array");
        setFindings(entries);
        setOpenId(entries[0]?.id ?? null);
      } catch (caught) {
        if (!cancelled) setError(caught instanceof Error ? caught.message : String(caught));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <section className="catalogue">
        <p className="error" role="alert">
          {error}
        </p>
      </section>
    );
  }
  if (!findings) return <section className="catalogue">Loading the catalogue…</section>;

  return (
    <section className="catalogue">
      <p className="lede">
        Real defects found by the campaigns, differential tests, and conformance gates — not a
        list padded to look thorough. Every entry pins the stream that produced it, and every
        stream can be opened in the projection room from here.
      </p>

      {findings.length === 0 ? (
        <p className="hint">
          The catalogue is empty. That is a finding of its own, and it is recorded rather than
          filled in.
        </p>
      ) : null}

      <ol className="findings">
        {findings.map((finding) => (
          <li key={finding.id} className="finding">
            <button
              type="button"
              className="finding-head"
              aria-expanded={openId === finding.id}
              onClick={() => setOpenId(openId === finding.id ? null : finding.id)}
            >
              <span className="finding-id">{finding.id}</span>
              <span className="finding-title">{finding.title}</span>
            </button>
            {openId === finding.id ? (
              <div className="finding-body">
                <dl>
                  <dt>Crate</dt>
                  <dd>{finding.crate}</dd>
                  <dt>Found by</dt>
                  <dd>{finding.foundBy}</dd>
                  <dt>Fixed in</dt>
                  <dd>{finding.fixedIn}</dd>
                  <dt>Regression</dt>
                  <dd>
                    <code>{finding.regression}</code>
                  </dd>
                </dl>
                <button
                  type="button"
                  className="open-regression"
                  onClick={() => onOpenStream(finding.stream, `regression ${finding.id}`)}
                >
                  Open this regression stream in the projection room
                </button>
                <Markdown text={finding.body} />
              </div>
            ) : null}
          </li>
        ))}
      </ol>
    </section>
  );
}

function Markdown({ text }: { text: string }) {
  return (
    <div className="prose">
      {parseMarkdown(text).map((block, index) => (
        <BlockView key={index} block={block} />
      ))}
    </div>
  );
}

function BlockView({ block }: { block: Block }) {
  switch (block.kind) {
    case "heading":
      return block.level === 2 ? (
        <h3>
          <Inlines inlines={block.inlines} />
        </h3>
      ) : (
        <h4>
          <Inlines inlines={block.inlines} />
        </h4>
      );
    case "list":
      return (
        <ul>
          {block.items.map((item, index) => (
            <li key={index}>
              <Inlines inlines={item} />
            </li>
          ))}
        </ul>
      );
    case "code":
      return (
        <pre>
          <code>{block.text}</code>
        </pre>
      );
    default:
      return (
        <p>
          <Inlines inlines={block.inlines} />
        </p>
      );
  }
}

function Inlines({ inlines }: { inlines: Inline[] }) {
  return (
    <>
      {inlines.map((inline, index) => {
        if (inline.kind === "code") return <code key={index}>{inline.text}</code>;
        if (inline.kind === "strong") return <strong key={index}>{inline.text}</strong>;
        return <span key={index}>{inline.text}</span>;
      })}
    </>
  );
}
