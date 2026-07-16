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
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, " ")
    .replace(/\s+/g, " ")
    .replace(/[. ]+$/g, "")
    .trim()
    .slice(0, 120);
  return `${base || "Documento"}.md`;
}
