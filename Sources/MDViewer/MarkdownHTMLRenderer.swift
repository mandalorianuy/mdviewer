import Down
import Foundation

enum MarkdownHTMLRenderer {
    static func renderDocument(markdown: String, fontFamily: String, baseFontSize: Double, appearanceMode: AppAppearanceMode) -> String {
        let body = renderBody(markdown)
        let safeFontFamily = cssEscaped(fontFamily)
        let bodySize = max(12, min(baseFontSize, 28))
        let h1Size = bodySize * 2.0
        let h2Size = bodySize * 1.6
        let h3Size = bodySize * 1.3

        return """
<!doctype html>
<html lang=\"es\" data-theme=\"\(appearanceMode.rawValue)\">
<head>
<meta charset=\"utf-8\" />
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
<meta name=\"color-scheme\" content=\"light dark\" />
<style>
\(themeStyles(fontFamily: safeFontFamily, bodySize: bodySize, h1Size: h1Size, h2Size: h2Size, h3Size: h3Size))
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
html {
  color-scheme: light dark;
}
body {
  background:
    radial-gradient(circle at 16% 10%, var(--glow-teal), transparent 24%),
    radial-gradient(circle at 88% 0%, var(--glow-violet), transparent 18%),
    radial-gradient(circle at 52% 100%, var(--glow-yellow), transparent 26%),
    linear-gradient(180deg, var(--bg), var(--bg-alt));
  color: var(--text);
  font-family: var(--font-body);
  font-size: var(--font-size);
  line-height: 1.7;
  padding: 14px;
}
.article {
  width: 100%;
  max-width: none;
  margin: 0;
  background: linear-gradient(180deg, var(--paper), color-mix(in oklab, var(--paper-alt) 88%, transparent));
  border: 1px solid var(--border-strong);
  border-radius: 18px;
  padding: 28px 30px;
  box-shadow: var(--paper-shadow);
  backdrop-filter: blur(10px);
  position: relative;
  overflow: hidden;
}
.article::before {
  content: "";
  position: absolute;
  top: 0;
  left: 30px;
  right: 30px;
  height: 2px;
  background: linear-gradient(90deg, var(--accent-yellow), var(--accent-teal));
  opacity: 0.9;
  border-radius: 999px;
}
h1, h2, h3, h4, h5, h6 {
  font-family: var(--font-display);
  line-height: 1.25;
  margin-top: 1.5em;
  margin-bottom: 0.6em;
  color: var(--text);
  letter-spacing: -0.02em;
}
h1 {
  font-size: var(--h1);
  margin-top: 0.2em;
  padding-bottom: 0.18em;
  border-bottom: none;
}
h1::after {
  content: "";
  display: block;
  width: min(220px, 38%);
  height: 2px;
  margin-top: 0.42em;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--accent-yellow), var(--accent-teal));
}
h2 {
  font-size: var(--h2);
  position: relative;
  padding-left: 0.72em;
}
h2::before {
  content: "";
  position: absolute;
  left: 0;
  top: 0.15em;
  bottom: 0.15em;
  width: 3px;
  border-radius: 999px;
  background: linear-gradient(180deg, var(--accent-teal), color-mix(in oklab, var(--accent-violet) 72%, var(--accent-teal)));
}
h3 {
  font-size: var(--h3);
  color: var(--text);
}
h4, h5, h6 {
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--muted);
  font-size: 0.78em;
}
p {
  margin: 0.85em 0;
  color: var(--text-secondary);
}
strong {
  color: var(--text);
  font-weight: 700;
}
em {
  font-style: italic;
}
del {
  color: var(--muted);
  text-decoration-thickness: 2px;
}
ul, ol {
  margin: 0.6em 0 1.1em 1.4em;
  padding-left: 1.1em;
  color: var(--text-secondary);
}
li { margin: 0.35em 0; }
li::marker { color: var(--marker); }
li > p:first-child {
  margin-top: 0;
}
li > p:last-child {
  margin-bottom: 0;
}
ul.contains-task-list,
ul.task-list {
  list-style: none;
  margin-left: 0;
  padding-left: 0.2em;
}
li.task-list-item {
  display: flex;
  align-items: flex-start;
  gap: 0.55em;
}
li.task-list-item input[type="checkbox"] {
  margin-top: 0.35em;
  flex: 0 0 auto;
}
blockquote {
  margin: 1.1em 0;
  padding: 0.9em 1.05em;
  border-left: 3px solid var(--quote-accent);
  color: var(--text);
  background: var(--quote-bg);
  border-radius: 12px;
}
blockquote p { margin: 0.4em 0; }
pre {
  margin: 1.1em 0;
  background: var(--code-bg);
  color: var(--code-text);
  border-radius: 10px;
  padding: 14px 16px;
  overflow-x: auto;
  font-size: 0.92em;
  line-height: 1.55;
  border: 1px solid var(--code-border);
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.03);
}
code {
  font-family: var(--font-mono);
}
pre code {
  background: transparent;
  color: inherit;
  border: none;
  padding: 0;
  font-size: 1em;
  white-space: pre;
}
p code, li code, blockquote code {
  background: var(--inline-code-bg);
  color: var(--inline-code-text);
  border: 1px solid var(--inline-code-border);
  border-radius: 7px;
  padding: 0.12em 0.42em;
  font-size: 0.92em;
}
a {
  color: var(--link);
  text-decoration: none;
  border-bottom: 1px solid color-mix(in oklab, var(--link) 32%, transparent);
  transition: color 140ms ease, border-color 140ms ease;
}
a:hover {
  color: var(--link-hover);
  border-bottom-color: color-mix(in oklab, var(--link-hover) 42%, transparent);
}
hr {
  border: none;
  border-top: 1px solid var(--border);
  margin: 1.6em 0;
}
table {
  border-collapse: collapse;
  width: max-content;
  min-width: 100%;
  margin: 0;
  table-layout: auto;
  background: transparent;
}
th, td {
  border: 1px solid var(--border);
  padding: 10px 12px;
  text-align: left;
  vertical-align: top;
  word-break: normal;
  overflow-wrap: anywhere;
  color: var(--text-secondary);
}
th {
  background: var(--table-head-bg);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 0.78em;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
thead th {
  font-weight: 700;
}
tbody tr:nth-child(even) {
  background: var(--table-row-alt);
}
td code, th code {
  background: var(--inline-code-bg);
  color: var(--inline-code-text);
  border: 1px solid var(--inline-code-border);
  border-radius: 6px;
  padding: 0.08em 0.4em;
  font-size: 0.92em;
  white-space: normal;
  word-break: break-word;
  overflow-wrap: anywhere;
}
.table-wrap {
  width: 100%;
  overflow-x: auto;
  margin: 1.2em 0;
  border: 1px solid var(--border-strong);
  border-radius: 12px;
  background: linear-gradient(180deg, color-mix(in oklab, var(--paper) 95%, transparent), color-mix(in oklab, var(--paper-alt) 94%, transparent));
  box-shadow: inset 0 1px 0 var(--surface-highlight);
}
.table-wrap table.resizable-table th,
.table-wrap table.resizable-table td {
  position: relative;
}
.table-wrap table.resizable-table th {
  user-select: none;
}
.column-resize-handle {
  position: absolute;
  top: 0;
  right: -5px;
  width: 10px;
  height: 100%;
  cursor: col-resize;
  z-index: 5;
}
.column-resize-handle::after {
  content: "";
  position: absolute;
  top: 22%;
  bottom: 22%;
  left: 50%;
  width: 2px;
  transform: translateX(-50%);
  border-radius: 999px;
  background: color-mix(in oklab, var(--link) 28%, transparent);
  transition: background 140ms ease;
}
.column-resize-handle:hover::after,
.column-resize-handle.is-active::after {
  background: var(--link);
}
input[type="checkbox"] {
  accent-color: var(--marker);
}
details {
  margin: 1em 0;
  padding: 0.85em 1em;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--quote-bg);
}
summary {
  cursor: pointer;
  font-weight: 600;
  color: var(--link);
}
section.footnotes {
  margin-top: 2.4em;
  padding-top: 1.2em;
  border-top: 1px solid var(--border);
  color: var(--text-secondary);
}
section.footnotes ol {
  margin-bottom: 0;
}
mark {
  background: var(--mark-bg);
  color: var(--mark-text);
  border-radius: 4px;
  padding: 0.05em 0.2em;
}
.mdviewer-find-hit {
  background: color-mix(in oklab, var(--accent-yellow) 18%, transparent);
  color: inherit;
  border-radius: 4px;
  box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--accent-yellow) 28%, transparent);
}
.mdviewer-find-hit.is-active {
  background: color-mix(in oklab, var(--accent-teal) 18%, transparent);
  box-shadow:
    inset 0 0 0 1px color-mix(in oklab, var(--accent-teal) 46%, transparent),
    0 0 0 2px color-mix(in oklab, var(--accent-teal) 14%, transparent);
}
kbd {
  font-family: var(--font-mono);
  font-size: 0.9em;
  background: var(--kbd-bg);
  border: 1px solid var(--kbd-border);
  border-bottom-width: 2px;
  border-radius: 6px;
  padding: 0.08em 0.38em;
  color: var(--text);
}
img {
  max-width: 100%;
  border-radius: 12px;
  border: 1px solid var(--border);
  box-shadow: 0 12px 30px rgba(0,0,0,0.12);
}
::-webkit-scrollbar {
  height: 10px;
  width: 10px;
}
::-webkit-scrollbar-track {
  background: transparent;
}
::-webkit-scrollbar-thumb {
  background: var(--scrollbar);
  border-radius: 999px;
}
::selection {
  background: color-mix(in oklab, var(--accent-yellow) 82%, white);
  color: #0F1117;
}
@media (max-width: 860px) {
  body { padding: 12px; }
  .article {
    padding: 20px 16px;
    border-radius: 10px;
  }
}
</style>
<script>
window.__mdviewerSearchController = (() => {
  const hitClass = "mdviewer-find-hit";
  const activeClass = "is-active";
  let hits = [];
  let activeIndex = -1;
  let lastQuery = "";

  function escapeRegExp(value) {
    return value.replace(/[.*+?^${}()|[\\]\\\\]/g, "\\\\$&");
  }

  function clearHighlights() {
    const existingHits = Array.from(document.querySelectorAll(`span.${hitClass}`));
    for (const hit of existingHits) {
      const parent = hit.parentNode;
      if (!parent) continue;
      parent.replaceChild(document.createTextNode(hit.textContent || ""), hit);
      parent.normalize();
    }
    hits = [];
    activeIndex = -1;
  }

  function collectTextNodes() {
    const article = document.querySelector(".article");
    if (!article) return [];

    const walker = document.createTreeWalker(
      article,
      NodeFilter.SHOW_TEXT,
      {
        acceptNode(node) {
          const parent = node.parentElement;
          if (!parent) return NodeFilter.FILTER_REJECT;
          if (["SCRIPT", "STYLE", "NOSCRIPT"].includes(parent.tagName)) return NodeFilter.FILTER_REJECT;
          if (parent.closest(`.${hitClass}`)) return NodeFilter.FILTER_REJECT;
          if (!(node.nodeValue || "").trim()) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        }
      }
    );

    const nodes = [];
    while (walker.nextNode()) {
      nodes.push(walker.currentNode);
    }
    return nodes;
  }

  function setActive(index) {
    if (hits.length === 0) {
      activeIndex = -1;
      return { currentIndex: 0, totalMatches: 0 };
    }

    activeIndex = ((index % hits.length) + hits.length) % hits.length;
    hits.forEach((hit, hitIndex) => {
      hit.classList.toggle(activeClass, hitIndex === activeIndex);
    });

    hits[activeIndex].scrollIntoView({
      block: "center",
      inline: "nearest",
      behavior: "smooth"
    });

    return {
      currentIndex: activeIndex + 1,
      totalMatches: hits.length
    };
  }

  function highlight(query) {
    clearHighlights();

    if (!query) {
      lastQuery = "";
      return { currentIndex: 0, totalMatches: 0 };
    }

    const regex = new RegExp(escapeRegExp(query), "gi");
    const textNodes = collectTextNodes();

    for (const node of textNodes) {
      const text = node.nodeValue || "";
      regex.lastIndex = 0;
      if (!regex.test(text)) continue;

      regex.lastIndex = 0;
      const fragment = document.createDocumentFragment();
      let lastIndex = 0;
      let match;

      while ((match = regex.exec(text)) !== null) {
        if (match.index > lastIndex) {
          fragment.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
        }

        const span = document.createElement("span");
        span.className = hitClass;
        span.textContent = match[0];
        fragment.appendChild(span);
        hits.push(span);
        lastIndex = match.index + match[0].length;
      }

      if (lastIndex < text.length) {
        fragment.appendChild(document.createTextNode(text.slice(lastIndex)));
      }

      node.parentNode.replaceChild(fragment, node);
    }

    lastQuery = query;
    return setActive(0);
  }

  function move(direction) {
    if (hits.length === 0) {
      return { currentIndex: 0, totalMatches: 0 };
    }

    const nextIndex = activeIndex < 0
      ? 0
      : activeIndex + direction;
    return setActive(nextIndex);
  }

  function search(query, action) {
    const normalizedQuery = query.trim();

    if (action === "clear" || normalizedQuery.length === 0) {
      clearHighlights();
      lastQuery = "";
      return { currentIndex: 0, totalMatches: 0 };
    }

    if (action === "update" || normalizedQuery !== lastQuery) {
      return highlight(normalizedQuery);
    }

    if (action === "previous") {
      return move(-1);
    }

    return move(1);
  }

  return { search };
})();

function setupResizableTables() {
  const tables = Array.from(document.querySelectorAll('.table-wrap table'));

  tables.forEach((table, tableIndex) => {
    const wrapper = table.closest('.table-wrap');
    const headerRow = table.tHead && table.tHead.rows.length > 0 ? table.tHead.rows[0] : table.rows[0];
    if (!wrapper || !headerRow || headerRow.cells.length === 0) {
      return;
    }

    table.classList.add('resizable-table');

    if (!table.querySelector('colgroup')) {
      const colgroup = document.createElement('colgroup');
      const headerCells = Array.from(headerRow.cells);
      headerCells.forEach((cell, cellIndex) => {
        const col = document.createElement('col');
        col.dataset.columnIndex = String(cellIndex);
        const width = Math.max(120, Math.ceil(cell.getBoundingClientRect().width));
        col.style.width = `${width}px`;
        colgroup.appendChild(col);
      });
      table.insertBefore(colgroup, table.firstChild);
    }

    const columns = Array.from(table.querySelectorAll('colgroup col'));
    if (columns.length === 0) {
      return;
    }

    fitTableToWrapper(table, wrapper, columns);

    Array.from(headerRow.cells).forEach((cell, columnIndex) => {
      if (cell.querySelector('.column-resize-handle')) {
        return;
      }

      const handle = document.createElement('div');
      handle.className = 'column-resize-handle';
      handle.title = 'Arrastrar para ajustar columna';

      handle.addEventListener('mousedown', (event) => {
        event.preventDefault();

        const startX = event.clientX;
        const column = columns[columnIndex];
        const startWidth = column.getBoundingClientRect().width;
        const minWidth = 90;

        handle.classList.add('is-active');

        const onMouseMove = (moveEvent) => {
          const nextWidth = Math.max(minWidth, startWidth + (moveEvent.clientX - startX));
          column.dataset.userSized = 'true';
          column.style.width = `${nextWidth}px`;
          table.style.width = `${Math.max(wrapper.clientWidth, totalColumnWidth(columns))}px`;
        };

        const onMouseUp = () => {
          handle.classList.remove('is-active');
          window.removeEventListener('mousemove', onMouseMove);
          window.removeEventListener('mouseup', onMouseUp);
        };

        window.addEventListener('mousemove', onMouseMove);
        window.addEventListener('mouseup', onMouseUp);
      });

      cell.appendChild(handle);
    });

    table.dataset.tableIndex = String(tableIndex);
    window.addEventListener('resize', () => fitTableToWrapper(table, wrapper, columns));
  });
}

function totalColumnWidth(columns) {
  return columns.reduce((sum, column) => sum + Math.max(90, Math.ceil(column.getBoundingClientRect().width)), 0);
}

function fitTableToWrapper(table, wrapper, columns) {
  const wrapperWidth = Math.max(0, Math.floor(wrapper.clientWidth));
  let totalWidth = totalColumnWidth(columns);

  if (wrapperWidth > totalWidth) {
    const flexibleColumns = columns.filter((column) => column.dataset.userSized !== 'true');
    const targets = flexibleColumns.length > 0 ? flexibleColumns : columns;
    const extraWidth = wrapperWidth - totalWidth;
    const perColumn = Math.floor(extraWidth / targets.length);
    let remainder = extraWidth - (perColumn * targets.length);

    targets.forEach((column) => {
      const currentWidth = Math.max(90, Math.ceil(column.getBoundingClientRect().width));
      const growth = perColumn + (remainder > 0 ? 1 : 0);
      remainder = Math.max(0, remainder - 1);
      column.style.width = `${currentWidth + growth}px`;
    });

    totalWidth = totalColumnWidth(columns);
  }

  table.style.width = `${Math.max(wrapperWidth, totalWidth)}px`;
}

window.addEventListener('load', setupResizableTables);
</script>
</head>
<body>
  <article class=\"article\">\(body)</article>
</body>
</html>
"""
    }

    private static func themeStyles(fontFamily: String, bodySize: Double, h1Size: Double, h2Size: Double, h3Size: Double) -> String {
        """
:root {
  --font-size: \(bodySize)px;
  --h1: \(h1Size)px;
  --h2: \(h2Size)px;
  --h3: \(h3Size)px;
  --font-body: "\(fontFamily)", "Space Grotesk", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-display: "Space Grotesk", "\(fontFamily)", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-mono: "JetBrains Mono", "SFMono-Regular", Menlo, Monaco, Consolas, monospace;
}
html[data-theme="dark"] {
  --bg: #0F1117;
  --bg-alt: #0A0C10;
  --paper: rgba(26, 29, 39, 0.96);
  --paper-alt: rgba(36, 39, 54, 0.98);
  --text: #E8EAF0;
  --text-secondary: #C1C5D2;
  --muted: #8B90A0;
  --disabled: #555A6E;
  --border: rgba(255,255,255,0.08);
  --border-strong: rgba(255,255,255,0.10);
  --accent-yellow: #FFEF34;
  --accent-teal: #00B8A3;
  --accent-violet: #8B5CF6;
  --marker: #00B8A3;
  --link: #00B8A3;
  --link-hover: #FFEF34;
  --quote-accent: #8B5CF6;
  --quote-bg: rgba(139,92,246,0.10);
  --table-head-bg: rgba(255,255,255,0.04);
  --table-row-alt: rgba(255,255,255,0.025);
  --inline-code-bg: rgba(36,39,54,0.95);
  --inline-code-text: #00B8A3;
  --inline-code-border: rgba(0,184,163,0.18);
  --code-bg: #0F1117;
  --code-text: #E8EAF0;
  --code-border: rgba(255,255,255,0.08);
  --kbd-bg: #242736;
  --kbd-border: rgba(255,255,255,0.14);
  --mark-bg: rgba(255,239,52,0.18);
  --mark-text: #0F1117;
  --scrollbar: rgba(85,90,110,0.64);
  --surface-highlight: rgba(255,255,255,0.04);
  --paper-shadow: 0 22px 60px rgba(0,0,0,0.34);
  --glow-yellow: rgba(255,239,52,0.06);
  --glow-violet: rgba(139,92,246,0.08);
  --glow-teal: rgba(0,184,163,0.10);
}
html[data-theme="light"] {
  --bg: #F5F5F0;
  --bg-alt: #F2F2F2;
  --paper: rgba(255,255,255,0.98);
  --paper-alt: rgba(234,234,229,0.96);
  --text: #1A1A1A;
  --text-secondary: #4A4A4A;
  --muted: #8B8B85;
  --disabled: #999999;
  --border: rgba(0,0,0,0.10);
  --border-strong: rgba(0,0,0,0.12);
  --accent-yellow: #D4C800;
  --accent-teal: #009688;
  --accent-violet: #7C3AED;
  --marker: #009688;
  --link: #009688;
  --link-hover: #7C3AED;
  --quote-accent: #7C3AED;
  --quote-bg: rgba(124,58,237,0.08);
  --table-head-bg: #EDEDEC;
  --table-row-alt: #F7F7F6;
  --inline-code-bg: rgba(0,150,136,0.10);
  --inline-code-text: #009688;
  --inline-code-border: rgba(0,150,136,0.16);
  --code-bg: #F0F0F0;
  --code-text: #30302D;
  --code-border: rgba(0,0,0,0.10);
  --kbd-bg: #F0F0F0;
  --kbd-border: rgba(0,0,0,0.12);
  --mark-bg: rgba(212,200,0,0.18);
  --mark-text: #1A1A1A;
  --scrollbar: rgba(74,74,74,0.26);
  --surface-highlight: rgba(255,255,255,0.72);
  --paper-shadow: 0 18px 42px rgba(0,0,0,0.08);
  --glow-yellow: rgba(212,200,0,0.07);
  --glow-violet: rgba(124,58,237,0.05);
  --glow-teal: rgba(0,150,136,0.06);
}
html[data-theme="system"] {
  --bg: #F5F5F0;
  --bg-alt: #F2F2F2;
  --paper: rgba(255,255,255,0.98);
  --paper-alt: rgba(234,234,229,0.96);
  --text: #1A1A1A;
  --text-secondary: #4A4A4A;
  --muted: #8B8B85;
  --disabled: #999999;
  --border: rgba(0,0,0,0.10);
  --border-strong: rgba(0,0,0,0.12);
  --accent-yellow: #D4C800;
  --accent-teal: #009688;
  --accent-violet: #7C3AED;
  --marker: #009688;
  --link: #009688;
  --link-hover: #7C3AED;
  --quote-accent: #7C3AED;
  --quote-bg: rgba(124,58,237,0.08);
  --table-head-bg: #EDEDEC;
  --table-row-alt: #F7F7F6;
  --inline-code-bg: rgba(0,150,136,0.10);
  --inline-code-text: #009688;
  --inline-code-border: rgba(0,150,136,0.16);
  --code-bg: #F0F0F0;
  --code-text: #30302D;
  --code-border: rgba(0,0,0,0.10);
  --kbd-bg: #F0F0F0;
  --kbd-border: rgba(0,0,0,0.12);
  --mark-bg: rgba(212,200,0,0.18);
  --mark-text: #1A1A1A;
  --scrollbar: rgba(74,74,74,0.26);
  --surface-highlight: rgba(255,255,255,0.72);
  --paper-shadow: 0 18px 42px rgba(0,0,0,0.08);
  --glow-yellow: rgba(212,200,0,0.07);
  --glow-violet: rgba(124,58,237,0.05);
  --glow-teal: rgba(0,150,136,0.06);
}
@media (prefers-color-scheme: dark) {
  html[data-theme="system"] {
    --bg: #0F1117;
    --bg-alt: #0A0C10;
    --paper: rgba(26, 29, 39, 0.96);
    --paper-alt: rgba(36, 39, 54, 0.98);
    --text: #E8EAF0;
    --text-secondary: #C1C5D2;
    --muted: #8B90A0;
    --disabled: #555A6E;
    --border: rgba(255,255,255,0.08);
    --border-strong: rgba(255,255,255,0.10);
    --accent-yellow: #FFEF34;
    --accent-teal: #00B8A3;
    --accent-violet: #8B5CF6;
    --marker: #00B8A3;
    --link: #00B8A3;
    --link-hover: #FFEF34;
    --quote-accent: #8B5CF6;
    --quote-bg: rgba(139,92,246,0.10);
    --table-head-bg: rgba(255,255,255,0.04);
    --table-row-alt: rgba(255,255,255,0.025);
    --inline-code-bg: rgba(36,39,54,0.95);
    --inline-code-text: #00B8A3;
    --inline-code-border: rgba(0,184,163,0.18);
    --code-bg: #0F1117;
    --code-text: #E8EAF0;
    --code-border: rgba(255,255,255,0.08);
    --kbd-bg: #242736;
    --kbd-border: rgba(255,255,255,0.14);
    --mark-bg: rgba(255,239,52,0.18);
    --mark-text: #0F1117;
    --scrollbar: rgba(85,90,110,0.64);
    --surface-highlight: rgba(255,255,255,0.04);
    --paper-shadow: 0 22px 60px rgba(0,0,0,0.34);
    --glow-yellow: rgba(255,239,52,0.06);
    --glow-violet: rgba(139,92,246,0.08);
    --glow-teal: rgba(0,184,163,0.10);
  }
}
"""
    }

    private static func renderBody(_ markdown: String) -> String {
        let preparedMarkdown = prepareMarkdown(markdown)

        do {
            let down = Down(markdownString: preparedMarkdown)
            let html = try down.toHTML(.unsafe)
            return html.replacingOccurrences(of: "<table>", with: "<div class=\"table-wrap\"><table>")
                .replacingOccurrences(of: "</table>", with: "</table></div>")
        } catch {
            return "<pre><code>\(htmlEscaped(preparedMarkdown))</code></pre>"
        }
    }

    private static func prepareMarkdown(_ markdown: String) -> String {
        let normalizedLineEndings = markdown.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        let repairedTables = repairCollapsedTables(in: normalizedLineEndings)
        return convertMarkdownTablesToHTML(in: repairedTables)
    }

    private static func repairCollapsedTables(in markdown: String) -> String {
        let lines = markdown.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var repairedLines: [String] = []

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            let looksLikeCollapsedTable = trimmed.hasPrefix("|") && trimmed.filter(\.isPipeCharacter).count >= 8 && trimmed.contains("||")

            guard looksLikeCollapsedTable else {
                repairedLines.append(line)
                continue
            }

            let segments = line
                .split(separator: "||", omittingEmptySubsequences: true)
                .map { normalizeCollapsedTableSegment(String($0)) }
                .filter { !$0.isEmpty }

            if segments.count >= 3, isMarkdownTableSeparator(segments[1]) {
                repairedLines.append(contentsOf: segments)
            } else {
                repairedLines.append(line)
            }
        }

        return repairedLines.joined(separator: "\n")
    }

    private static func normalizeCollapsedTableSegment(_ segment: String) -> String {
        var normalized = segment.trimmingCharacters(in: .whitespacesAndNewlines)

        if !normalized.hasPrefix("|") {
            normalized = "|" + normalized
        }

        if !normalized.hasSuffix("|") {
            normalized += "|"
        }

        return normalized
    }

    private static func isMarkdownTableSeparator(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("|"), trimmed.hasSuffix("|") else {
            return false
        }

        let cells = trimmed
            .dropFirst()
            .dropLast()
            .split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }

        guard !cells.isEmpty else {
            return false
        }

        return cells.allSatisfy { cell in
            !cell.isEmpty && cell.allSatisfy { $0 == "-" || $0 == ":" }
        }
    }

    private static func htmlEscaped(_ input: String) -> String {
        input
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&#39;")
    }

    private static func cssEscaped(_ input: String) -> String {
        input
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
    }

    private static func convertMarkdownTablesToHTML(in markdown: String) -> String {
        let lines = markdown.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var output: [String] = []
        var index = 0
        var activeFence: String?

        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if let fence = markdownFenceMarker(in: trimmed) {
                if activeFence == fence {
                    activeFence = nil
                } else if activeFence == nil {
                    activeFence = fence
                }
                output.append(line)
                index += 1
                continue
            }

            guard activeFence == nil,
                  index + 1 < lines.count,
                  isPotentialTableRow(line),
                  isMarkdownTableSeparator(lines[index + 1]) else {
                output.append(line)
                index += 1
                continue
            }

            var tableLines = [line, lines[index + 1]]
            index += 2

            while index < lines.count, isPotentialTableRow(lines[index]) {
                tableLines.append(lines[index])
                index += 1
            }

            output.append(renderTableHTML(from: tableLines))
        }

        return output.joined(separator: "\n")
    }

    private static func renderTableHTML(from lines: [String]) -> String {
        guard lines.count >= 2 else {
            return lines.joined(separator: "\n")
        }

        let headerCells = splitTableRow(lines[0])
        let alignments = splitTableRow(lines[1]).map(tableAlignment(for:))
        let bodyRows = lines.dropFirst(2).map(splitTableRow)

        guard headerCells.count >= 2, alignments.count == headerCells.count else {
            return lines.joined(separator: "\n")
        }

        func inlineHTML(for cell: String) -> String {
            let trimmed = cell.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else {
                return ""
            }

            do {
                let rendered = try Down(markdownString: trimmed).toHTML(.unsafe)
                return stripOuterParagraph(from: rendered)
            } catch {
                return htmlEscaped(trimmed)
            }
        }

        func styleAttribute(for alignment: TableAlignment) -> String {
            switch alignment {
            case .left:
                return " style=\"text-align: left;\""
            case .center:
                return " style=\"text-align: center;\""
            case .right:
                return " style=\"text-align: right;\""
            case .none:
                return ""
            }
        }

        let headerHTML = zip(headerCells, alignments).map { cell, alignment in
            "<th\(styleAttribute(for: alignment))>\(inlineHTML(for: cell))</th>"
        }.joined()

        let rowsHTML = bodyRows.map { row in
            let paddedRow = row.count < alignments.count ? row + Array(repeating: "", count: alignments.count - row.count) : Array(row.prefix(alignments.count))
            let cellsHTML = zip(paddedRow, alignments).map { cell, alignment in
                "<td\(styleAttribute(for: alignment))>\(inlineHTML(for: cell))</td>"
            }.joined()
            return "<tr>\(cellsHTML)</tr>"
        }.joined(separator: "\n")

        var sections = ["<table>", "<thead><tr>\(headerHTML)</tr></thead>"]
        if !rowsHTML.isEmpty {
            sections.append("<tbody>\n\(rowsHTML)\n</tbody>")
        }
        sections.append("</table>")

        return sections.joined(separator: "\n")
    }

    private static func splitTableRow(_ line: String) -> [String] {
        var trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.hasPrefix("|") {
            trimmed.removeFirst()
        }
        if trimmed.hasSuffix("|") {
            trimmed.removeLast()
        }

        var cells: [String] = []
        var current = ""
        var isEscaped = false

        for character in trimmed {
            if isEscaped {
                current.append(character)
                isEscaped = false
                continue
            }

            if character == "\\" {
                current.append(character)
                isEscaped = true
                continue
            }

            if character == "|" {
                cells.append(current.trimmingCharacters(in: .whitespaces))
                current = ""
            } else {
                current.append(character)
            }
        }

        cells.append(current.trimmingCharacters(in: .whitespaces))
        return cells
    }

    private static func isPotentialTableRow(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.contains("|"), !trimmed.isEmpty else {
            return false
        }

        let cells = splitTableRow(trimmed)
        return cells.count >= 2 && cells.contains { !$0.isEmpty }
    }

    private static func markdownFenceMarker(in line: String) -> String? {
        if line.hasPrefix("```") {
            return "```"
        }

        if line.hasPrefix("~~~") {
            return "~~~"
        }

        return nil
    }

    private static func stripOuterParagraph(from html: String) -> String {
        let trimmed = html.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("<p>"), trimmed.hasSuffix("</p>") else {
            return trimmed
        }

        return String(trimmed.dropFirst(3).dropLast(4))
    }

    private static func tableAlignment(for cell: String) -> TableAlignment {
        let trimmed = cell.trimmingCharacters(in: .whitespacesAndNewlines)
        let isValidSeparator = !trimmed.isEmpty && trimmed.allSatisfy { $0 == "-" || $0 == ":" }

        guard isValidSeparator else {
            return .none
        }

        let hasLeadingColon = trimmed.hasPrefix(":")
        let hasTrailingColon = trimmed.hasSuffix(":")

        switch (hasLeadingColon, hasTrailingColon) {
        case (true, true):
            return .center
        case (false, true):
            return .right
        case (true, false):
            return .left
        case (false, false):
            return .none
        }
    }
}

private extension Character {
    var isPipeCharacter: Bool {
        self == "|"
    }
}

private enum TableAlignment {
    case none
    case left
    case center
    case right
}
