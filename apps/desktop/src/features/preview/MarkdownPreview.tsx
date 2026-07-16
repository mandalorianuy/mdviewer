import { createElement, Fragment, type MouseEvent, type ReactNode } from "react";

interface MarkdownPreviewProps {
  markdown: string;
  onExternalLink(url: string): void;
}

function cleanSource(source: string): string {
  return source
    .replace(/<script\b[^>]*>[\s\S]*?<\/script\s*>/gi, "")
    .replace(/<style\b[^>]*>[\s\S]*?<\/style\s*>/gi, "")
    .replace(/<iframe\b[^>]*>[\s\S]*?<\/iframe\s*>/gi, "")
    .replace(/<[^>]+>/g, "");
}

function isExternal(url: string): boolean {
  return /^https?:\/\//i.test(url);
}

function isSafeLocal(url: string): boolean {
  return url.startsWith("#") || /^(?![a-z][a-z\d+.-]*:)[^\\]*$/i.test(url);
}

function inline(value: string, onExternalLink: (url: string) => void): ReactNode[] {
  const source = cleanSource(value);
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^\s)]+\))/g;
  return source.split(pattern).filter(Boolean).map((part, index) => {
    if (part.startsWith("`") && part.endsWith("`")) return <code key={index}>{part.slice(1, -1)}</code>;
    if (part.startsWith("**") && part.endsWith("**")) return <strong key={index}>{part.slice(2, -2)}</strong>;
    if (part.startsWith("*") && part.endsWith("*")) return <em key={index}>{part.slice(1, -1)}</em>;
    const link = /^\[([^\]]+)\]\(([^\s)]+)\)$/.exec(part);
    if (link) {
      const [, label, url] = link;
      if (isExternal(url)) {
        const follow = (event: MouseEvent<HTMLAnchorElement>) => {
          event.preventDefault();
          onExternalLink(url);
        };
        return <a key={index} href={url} onClick={follow}>{label}</a>;
      }
      if (isSafeLocal(url)) return <a key={index} href={url}>{label}</a>;
      return <Fragment key={index}>{label}</Fragment>;
    }
    return <Fragment key={index}>{part}</Fragment>;
  });
}

function table(lines: string[], onExternalLink: (url: string) => void): ReactNode {
  const rows = lines.map((line) => line.replace(/^\||\|$/g, "").split("|").map((cell) => cell.trim()));
  return (
    <table>
      <thead><tr>{rows[0].map((cell, i) => <th key={i}>{inline(cell, onExternalLink)}</th>)}</tr></thead>
      <tbody>{rows.slice(2).map((row, i) => <tr key={i}>{row.map((cell, j) => <td key={j}>{inline(cell, onExternalLink)}</td>)}</tr>)}</tbody>
    </table>
  );
}

function blocks(markdown: string, onExternalLink: (url: string) => void): ReactNode[] {
  const lines = cleanSource(markdown).replace(/\r\n?/g, "\n").split("\n");
  const result: ReactNode[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) { index += 1; continue; }
    if (line.startsWith("```")) {
      const language = line.slice(3).trim();
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].startsWith("```")) code.push(lines[index++]);
      index += 1;
      result.push(<pre key={result.length}><code data-language={language}>{code.join("\n")}</code></pre>);
      continue;
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      const id = heading[2].toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "");
      result.push(createElement(`h${level}`, { id, key: result.length }, inline(heading[2], onExternalLink)));
      index += 1;
      continue;
    }
    if (line.includes("|") && index + 1 < lines.length && /^\s*\|?\s*:?-{3,}/.test(lines[index + 1])) {
      const tableLines = [line, lines[index + 1]];
      index += 2;
      while (index < lines.length && lines[index].includes("|")) tableLines.push(lines[index++]);
      result.push(<Fragment key={result.length}>{table(tableLines, onExternalLink)}</Fragment>);
      continue;
    }
    const task = /^\s*[-*+]\s+\[([ xX])\]\s+(.+)$/.exec(line);
    if (task) {
      const tasks: Array<{ checked: boolean; label: string }> = [];
      while (index < lines.length) {
        const match = /^\s*[-*+]\s+\[([ xX])\]\s+(.+)$/.exec(lines[index]);
        if (!match) break;
        tasks.push({ checked: match[1].toLowerCase() === "x", label: match[2] });
        index += 1;
      }
      result.push(<ul className="task-list" key={result.length}>{tasks.map((item, i) => <li key={i}><label><input type="checkbox" checked={item.checked} readOnly /> {item.label}</label></li>)}</ul>);
      continue;
    }
    if (/^\s*[-*+]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*[-*+]\s+/.test(lines[index])) items.push(lines[index++].replace(/^\s*[-*+]\s+/, ""));
      result.push(<ul key={result.length}>{items.map((item, i) => <li key={i}>{inline(item, onExternalLink)}</li>)}</ul>);
      continue;
    }
    if (/^>\s?/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^>\s?/.test(lines[index])) quote.push(lines[index++].replace(/^>\s?/, ""));
      result.push(<blockquote key={result.length}>{inline(quote.join(" "), onExternalLink)}</blockquote>);
      continue;
    }
    const paragraph = [line];
    index += 1;
    while (index < lines.length && lines[index].trim() && !/^(#{1,6})\s|^```|^\s*[-*+]\s|^>\s?/.test(lines[index])) paragraph.push(lines[index++]);
    result.push(<p key={result.length}>{inline(paragraph.join(" "), onExternalLink)}</p>);
  }
  return result;
}

export function MarkdownPreview({ markdown, onExternalLink }: MarkdownPreviewProps) {
  return <section className="preview-pane markdown-body" aria-label="Vista previa">{blocks(markdown, onExternalLink)}</section>;
}
