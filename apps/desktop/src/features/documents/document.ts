export interface DocumentState {
  name: string;
  content: string;
  savedContent: string;
  writeToken?: string;
}

export const untitledDocument: DocumentState = {
  name: "Sin título.md",
  content: "",
  savedContent: "",
};

export function markdownName(title: string): string {
  const base = title
    .normalize("NFKC")
    .replace(/\.md$/i, "")
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, " ")
    .replace(/\s+/g, " ")
    .replace(/[. ]+$/g, "")
    .trim();
  const stem = base || "Documento";
  const graphemes = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(stem)]
    .map((segment) => segment.segment)
    .slice(0, 117)
    .join("");
  return `${graphemes}.md`;
}
