const STANDALONE_STYLES = `
:root { color-scheme: light dark; }
body { margin: 0; padding: 2rem; color: #172033; background: #fff; }
.markdown-body { max-width: 54rem; margin: 0 auto; font: 15px/1.7 ui-serif, Georgia, Cambria, serif; }
.markdown-body h1, .markdown-body h2, .markdown-body h3, .markdown-body h4 { font-family: ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; line-height: 1.22; }
.markdown-body a { color: #2752bc; }
.markdown-body code { padding: 2px 5px; border-radius: 4px; background: #f0f2f5; font: .9em ui-monospace, monospace; }
.markdown-body pre { overflow: auto; padding: 15px; border: 1px solid #d9dee8; border-radius: 8px; background: #f0f2f5; }
.markdown-body pre code { padding: 0; background: transparent; }
.markdown-body blockquote { margin-left: 0; padding-left: 16px; border-left: 3px solid #3566dc; color: #667085; }
.markdown-body table { width: 100%; border-collapse: collapse; margin: 18px 0; font-family: ui-sans-serif, sans-serif; font-size: 13px; }
.markdown-body th, .markdown-body td { padding: 8px 10px; border: 1px solid #d9dee8; text-align: left; }
.markdown-body img { max-width: 100%; height: auto; }
@media print { body { padding: 0; } .markdown-body { max-width: none; } }
`.trim();

const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES = 20 * 1024 * 1024;
const MAX_IMAGES = 100;
const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

export const PRINT_STYLESHEET = `
:root[data-mdviewer-printing="true"] {
  color-scheme: light;
  --canvas: #fff;
  --surface: #fff;
  --surface-muted: #f3f4f6;
  --text: #000;
  --muted: #374151;
  --line: #d1d5db;
  --accent: #000;
  --accent-strong: #000;
}
:root[data-mdviewer-printing="true"] .app-header,
:root[data-mdviewer-printing="true"] .toolbar,
:root[data-mdviewer-printing="true"] .document-bar,
:root[data-mdviewer-printing="true"] .integrations-panel,
:root[data-mdviewer-printing="true"] .conversion-status,
:root[data-mdviewer-printing="true"] .conversion-notice,
:root[data-mdviewer-printing="true"] .warning-list,
:root[data-mdviewer-printing="true"] .error-banner,
:root[data-mdviewer-printing="true"] .editor-pane,
:root[data-mdviewer-printing="true"] .status-bar { display: none !important; }
:root[data-mdviewer-printing="true"] .workspace { display: block !important; }
:root[data-mdviewer-printing="true"] .preview-pane {
  display: block !important;
  overflow: visible !important;
  padding: 0 !important;
  color: #000 !important;
  background: #fff !important;
}
:root[data-mdviewer-printing="true"] .preview-pane pre,
:root[data-mdviewer-printing="true"] .preview-pane code,
:root[data-mdviewer-printing="true"] .preview-pane th {
  color: #000 !important;
  background: #f3f4f6 !important;
}
:root[data-mdviewer-printing="true"] .preview-pane td {
  color: #000 !important;
  background: #fff !important;
}
@media print {
  @page { margin: 14mm; }
  :root {
    color-scheme: light;
    --canvas: #fff;
    --surface: #fff;
    --surface-muted: #f3f4f6;
    --text: #000;
    --muted: #374151;
    --line: #d1d5db;
    --accent: #000;
    --accent-strong: #000;
  }
  html, body, #root, .app-shell, .workspace { min-width: 0 !important; width: auto !important; height: auto !important; min-height: 0 !important; overflow: visible !important; background: #fff !important; }
  .app-header, .toolbar, .document-bar, .integrations-panel, .conversion-status, .conversion-notice, .warning-list, .error-banner, .editor-pane, .status-bar { display: none !important; }
  .workspace { display: block !important; }
  .preview-pane { display: block !important; overflow: visible !important; padding: 0 !important; color: #000 !important; background: #fff !important; }
  .preview-pane a { color: #000 !important; text-decoration: underline; }
  .preview-pane pre, .preview-pane code, .preview-pane th { color: #000 !important; background: #f3f4f6 !important; }
  .preview-pane td { color: #000 !important; background: #fff !important; }
  .preview-pane pre, .preview-pane table, .preview-pane img { break-inside: avoid; }
}
`.trim();

const CONTENT_SECURITY_POLICY = "default-src 'none'; base-uri 'none'; form-action 'none'; connect-src 'none'; object-src 'none'; frame-src 'none'; media-src 'none'; img-src data:; style-src 'unsafe-inline'";
const DANGEROUS_ELEMENTS = "script, style, iframe, object, embed, applet, link, meta, base, form, audio, video, source, track, svg, use";
const URL_OR_CSS_ATTRIBUTES = new Set([
  "action",
  "archive",
  "background",
  "cite",
  "codebase",
  "data",
  "dynsrc",
  "formaction",
  "icon",
  "imagesrcset",
  "longdesc",
  "lowsrc",
  "manifest",
  "ping",
  "poster",
  "profile",
  "srcdoc",
  "srcset",
  "style",
  "usemap",
  "xlink:href",
]);

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function safeHref(value: string): boolean {
  if (/^#[\p{L}\p{N}_.:-]+$/u.test(value)) return true;
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:")
      && url.host.length > 0
      && url.username.length === 0
      && url.password.length === 0
      && (value.startsWith("http://") || value.startsWith("https://"));
  } catch {
    return false;
  }
}

function safeImageSource(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "asset:" && url.host === "localhost";
  } catch {
    return false;
  }
}

function base64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

async function readBoundedImage(response: Response, maximumBytes: number): Promise<Uint8Array> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error("export image unavailable");
  const chunks: Uint8Array[] = [];
  let totalBytes = 0;

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (totalBytes + value.byteLength > maximumBytes) {
        await reader.cancel();
        throw new Error("export image is too large");
      }
      chunks.push(value);
      totalBytes += value.byteLength;
    }
  } finally {
    reader.releaseLock();
  }

  const bytes = new Uint8Array(totalBytes);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

async function inlineLocalImages(clone: HTMLElement): Promise<void> {
  const images = [...clone.querySelectorAll<HTMLImageElement>("img[src]")];
  if (images.length > MAX_IMAGES) throw new Error("too many export images");
  const cache = new Map<string, { dataUrl: string; bytes: number }>();
  let totalBytes = 0;

  for (const image of images) {
    const source = image.getAttribute("src");
    if (!source || !safeImageSource(source)) throw new Error("unsafe export image source");
    let encoded = cache.get(source);
    if (!encoded) {
      const response = await fetch(source);
      if (!response.ok) throw new Error("export image unavailable");
      const type = response.headers.get("content-type")?.split(";", 1)[0].trim().toLocaleLowerCase() ?? "";
      if (!IMAGE_TYPES.has(type)) throw new Error("unsupported export image type");
      const declaredLength = response.headers.get("content-length");
      if (declaredLength !== null) {
        const parsedLength = Number(declaredLength);
        if (!Number.isSafeInteger(parsedLength) || parsedLength < 0 || parsedLength > MAX_IMAGE_BYTES) {
          throw new Error("export image is too large");
        }
      }
      const bytes = await readBoundedImage(response, MAX_IMAGE_BYTES);
      encoded = { dataUrl: `data:${type};base64,${base64(bytes)}`, bytes: bytes.byteLength };
      cache.set(source, encoded);
    }
    if (totalBytes + encoded.bytes > MAX_TOTAL_IMAGE_BYTES) throw new Error("export images are too large");
    totalBytes += encoded.bytes;
    image.setAttribute("src", encoded.dataUrl);
  }
}

async function sanitizedPreviewMarkup(preview: HTMLElement): Promise<string> {
  const clone = preview.cloneNode(true) as HTMLElement;
  clone.querySelectorAll(DANGEROUS_ELEMENTS).forEach((node) => node.remove());
  clone.querySelectorAll<HTMLElement>("*").forEach((element) => {
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLocaleLowerCase();
      if (name.startsWith("on") || URL_OR_CSS_ATTRIBUTES.has(name)) element.removeAttribute(attribute.name);
    }
    const exportHref = element.getAttribute("data-export-href");
    element.removeAttribute("data-export-href");
    if (element.tagName === "A" && exportHref !== null && safeHref(exportHref)) element.setAttribute("href", exportHref);
    const href = element.getAttribute("href");
    if (href !== null && (element.tagName !== "A" || !safeHref(href))) element.removeAttribute("href");
    const src = element.getAttribute("src");
    if (src !== null && (element.tagName !== "IMG" || !safeImageSource(src))) element.removeAttribute("src");
    const sanitizedHref = element.getAttribute("href");
    if (element.tagName === "A" && sanitizedHref !== null && !sanitizedHref.startsWith("#") && safeHref(sanitizedHref)) {
      element.setAttribute("rel", "noopener noreferrer");
    }
  });
  await inlineLocalImages(clone);
  return clone.innerHTML;
}

export async function buildStandaloneHtml(preview: HTMLElement, title: string): Promise<string> {
  const markup = await sanitizedPreviewMarkup(preview);
  return [
    "<!doctype html>",
    '<html lang="es">',
    "<head>",
    '<meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<meta http-equiv="Content-Security-Policy" content="${CONTENT_SECURITY_POLICY}">`,
    `<title>${escapeHtml(title)}</title>`,
    `<style>${STANDALONE_STYLES}</style>`,
    "</head>",
    "<body>",
    `<main class="markdown-body">${markup}</main>`,
    "</body>",
    "</html>",
    "",
  ].join("\n");
}

export function htmlExportName(documentName: string): string {
  const stem = documentName.replace(/\.md$/i, "").trim() || "Documento";
  return `${stem}.html`;
}
