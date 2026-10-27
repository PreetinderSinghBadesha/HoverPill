import { Fragment, useEffect, useRef, useState, type ReactNode } from "react";
import { copyText, openLink } from "../lib/bridge";
import { Icon } from "./icons";

/**
 * Just enough Markdown for ChatGPT's answers: paragraphs, headings, lists, code, tables, bold,
 * italics and links. Builds React elements (never HTML), and copes with a half-streamed answer:
 * an unclosed code block or table simply ends where the text does so far.
 */

type Block =
  | { kind: "p"; lines: string[] }
  | { kind: "heading"; text: string }
  | { kind: "code"; lang: string; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: string[] }
  | { kind: "table"; head: string[]; rows: string[][] }
  | { kind: "rule" };

const FENCE = /^\s*```/;
const HEADING = /^\s*#{1,6}\s+/;
const RULE = /^\s*(-{3,}|\*{3,}|_{3,})\s*$/;
const ITEM = /^\s*([-*+]|\d+[.)])\s+(.*)$/;
const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

const isTable = (lines: string[], i: number) => lines[i].trim().startsWith("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]);
const startsBlock = (lines: string[], i: number) =>
  FENCE.test(lines[i]) || HEADING.test(lines[i]) || RULE.test(lines[i]) || ITEM.test(lines[i]) || isTable(lines, i);
const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());

export function parse(text: string): Block[] {
  const lines = text.replace(/\r/g, "").split("\n");
  const blocks: Block[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
    } else if (FENCE.test(line)) {
      const lang = line.trim().slice(3).trim();
      const body: string[] = [];
      i++;
      while (i < lines.length && !FENCE.test(lines[i])) body.push(lines[i++]);
      i++; // the closing fence, if it has arrived
      blocks.push({ kind: "code", lang, text: body.join("\n") });
    } else if (HEADING.test(line)) {
      blocks.push({ kind: "heading", text: line.replace(HEADING, "") });
      i++;
    } else if (RULE.test(line)) {
      blocks.push({ kind: "rule" });
      i++;
    } else if (isTable(lines, i)) {
      const head = cells(line);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i].trim().startsWith("|")) rows.push(cells(lines[i++]));
      blocks.push({ kind: "table", head, rows });
    } else if (ITEM.test(line)) {
      const ordered = /\d/.test(ITEM.exec(line)![1]);
      const items: string[] = [];
      let match: RegExpExecArray | null;
      while (i < lines.length && (match = ITEM.exec(lines[i])) && /\d/.test(match[1]) === ordered) {
        items.push(match[2]);
        i++;
      }
      blocks.push({ kind: "list", ordered, start: ordered ? parseInt(ITEM.exec(line)![1], 10) || 1 : 1, items });
    } else {
      // Always take this line, so a line that only looks like a block start can't stall the loop.
      const para = [lines[i++]];
      while (i < lines.length && lines[i].trim() && !startsBlock(lines, i)) para.push(lines[i++]);
      blocks.push({ kind: "p", lines: para });
    }
  }
  return blocks;
}

/** `code`, **bold**, *italic* and [links](https://...) within a line. */
function inline(text: string): ReactNode[] {
  const pattern = /(`[^`]+`)|(\*\*[^*]+\*\*|__[^_]+__)|(\*[^*\s][^*]*\*)|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g;
  const out: ReactNode[] = [];
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = pattern.exec(text))) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const key = out.length;
    if (m[1]) out.push(<code key={key}>{m[1].slice(1, -1)}</code>);
    else if (m[2]) out.push(<strong key={key}>{inline(m[2].slice(2, -2))}</strong>);
    else if (m[3]) out.push(<em key={key}>{m[3].slice(1, -1)}</em>);
    else {
      const url = m[5];
      out.push(
        <a
          key={key}
          href={url}
          onClick={(e) => {
            e.preventDefault();
            openLink(url);
          }}
        >
          {m[4]}
        </a>,
      );
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

/** Copies `text`, then says so for a moment. */
export function CopyButton({ text, label, className = "md-copy" }: { text: string; label: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const copy = () => {
    copyText(text)
      .then(() => {
        setCopied(true);
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(console.error);
  };
  return (
    <button type="button" className={`${className}${copied ? " is-copied" : ""}`} aria-label={label} title={label} onClick={copy}>
      <Icon name={copied ? "check" : "copy"} />
      <span>{copied ? "Copied" : "Copy"}</span>
    </button>
  );
}

/** `trailing` (the typing caret) goes at the end of the last block. */
export function Markdown({ text, trailing }: { text: string; trailing?: ReactNode }) {
  const blocks = parse(text);
  return (
    <div className="md">
      {blocks.map((b, i) => {
        const end = i === blocks.length - 1 ? trailing : null;
        switch (b.kind) {
          case "p":
            return (
              <p key={i}>
                {b.lines.map((line, j) => (
                  <Fragment key={j}>
                    {j > 0 && <br />}
                    {inline(line)}
                  </Fragment>
                ))}
                {end}
              </p>
            );
          case "heading":
            return (
              <p key={i} className="md-heading">
                {inline(b.text)}
                {end}
              </p>
            );
          case "code":
            return (
              <Fragment key={i}>
                <div className="md-code">
                  <div className="md-code-head">
                    <span>{b.lang}</span>
                    <CopyButton text={b.text} label="Copy code" />
                  </div>
                  <pre>
                    <code>{b.text}</code>
                  </pre>
                </div>
                {end}
              </Fragment>
            );
          case "list": {
            const items = b.items.map((item, j) => (
              <li key={j}>
                {inline(item)}
                {j === b.items.length - 1 && end}
              </li>
            ));
            return b.ordered ? (
              <ol key={i} start={b.start}>
                {items}
              </ol>
            ) : (
              <ul key={i}>{items}</ul>
            );
          }
          case "table":
            return (
              <Fragment key={i}>
                <div className="md-table">
                  <table>
                    <thead>
                      <tr>
                        {b.head.map((c, j) => (
                          <th key={j}>{inline(c)}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {b.rows.map((row, r) => (
                        <tr key={r}>
                          {row.map((c, j) => (
                            <td key={j}>{inline(c)}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                {end}
              </Fragment>
            );
          case "rule":
            return (
              <Fragment key={i}>
                <hr />
                {end}
              </Fragment>
            );
        }
      })}
      {blocks.length === 0 && trailing}
    </div>
  );
}
