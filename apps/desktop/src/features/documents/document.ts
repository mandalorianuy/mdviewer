export interface DocumentState {
  generation: number;
  name: string;
  content: string;
  savedContent: string;
  writeToken?: string;
}

export const untitledDocument: DocumentState = {
  generation: 0,
  name: "Sin título.md",
  content: "",
  savedContent: "",
};

export interface DocumentSnapshot {
  generation: number;
  content: string;
}

export interface SaveCompletion {
  generation: number;
  savedContent: string;
  writeToken: string;
  name?: string;
}

export type DocumentTransition =
  | { type: "save-completed"; completion: SaveCompletion }
  | { type: "replace"; replacement: DocumentState }
  | { type: "conversion-completed"; accepted: false }
  | { type: "conversion-completed"; accepted: true; replacement: DocumentState };

export function applySaveCompletion(
  current: DocumentState,
  completion: SaveCompletion,
): DocumentState {
  if (current.generation !== completion.generation) return current;
  return {
    ...current,
    ...(completion.name === undefined ? {} : { name: completion.name }),
    savedContent: completion.savedContent,
    writeToken: completion.writeToken,
  };
}

export function transitionDocument(
  current: DocumentState,
  transition: DocumentTransition,
): DocumentState {
  switch (transition.type) {
    case "save-completed":
      return applySaveCompletion(current, transition.completion);
    case "replace":
      return transition.replacement;
    case "conversion-completed":
      return transition.accepted ? transition.replacement : current;
  }
}

export function replacementPrompt(
  current: DocumentState,
  snapshot: DocumentSnapshot,
  isDirty: boolean,
): string | undefined {
  if (current.generation !== snapshot.generation) {
    return "El documento cambió mientras se convertía. ¿Reemplazarlo con la conversión?";
  }
  if (isDirty && current.content !== snapshot.content) {
    return "Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?";
  }
  return undefined;
}

export function confirmReplacement(
  current: DocumentState,
  snapshot: DocumentSnapshot,
  isDirty: boolean,
  confirm: (message: string) => boolean,
): boolean {
  const message = replacementPrompt(current, snapshot, isDirty);
  return message === undefined || confirm(message);
}

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
