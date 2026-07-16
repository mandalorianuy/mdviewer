import { useEffect, useMemo, useRef, useState } from "react";

interface EditorSurfaceProps {
  content: string;
  findOpen: boolean;
  findQuery: string;
  onChange(content: string): void;
  onFindChange(query: string): void;
  onFindClose(): void;
}

export function EditorSurface({
  content,
  findOpen,
  findQuery,
  onChange,
  onFindChange,
  onFindClose,
}: EditorSurfaceProps) {
  const searchRef = useRef<HTMLInputElement>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const [currentMatch, setCurrentMatch] = useState(0);
  useEffect(() => {
    if (findOpen) searchRef.current?.focus();
  }, [findOpen]);

  const matches = useMemo(() => {
    if (!findQuery) return [];
    const positions: number[] = [];
    const source = content.toLocaleLowerCase();
    const query = findQuery.toLocaleLowerCase();
    let offset = 0;
    while (offset <= source.length - query.length) {
      const position = source.indexOf(query, offset);
      if (position < 0) break;
      positions.push(position);
      offset = position + Math.max(query.length, 1);
    }
    return positions;
  }, [content, findQuery]);

  useEffect(() => setCurrentMatch(0), [findQuery]);

  useEffect(() => {
    if (matches.length === 0) return;
    const index = Math.min(currentMatch, matches.length - 1);
    editorRef.current?.setSelectionRange(matches[index], matches[index] + findQuery.length);
    if (index !== currentMatch) setCurrentMatch(index);
  }, [currentMatch, findQuery, matches]);

  const moveMatch = (direction: 1 | -1) => {
    if (matches.length === 0) return;
    setCurrentMatch((current) => (current + direction + matches.length) % matches.length);
  };

  return (
    <section className="editor-pane" aria-label="Editor">
      {findOpen && (
        <div className="find-bar" role="search">
          <input
            ref={searchRef}
            type="search"
            aria-label="Buscar en el documento"
            value={findQuery}
            onChange={(event) => onFindChange(event.target.value)}
            onKeyDown={(event) => event.key === "Escape" && onFindClose()}
          />
          <span aria-live="polite">
            {matches.length > 0
              ? `${currentMatch + 1} de ${matches.length} ${matches.length === 1 ? "coincidencia" : "coincidencias"}`
              : "0 coincidencias"}
          </span>
          <button type="button" className="quiet" onClick={() => moveMatch(-1)} disabled={matches.length === 0} aria-label="Coincidencia anterior">↑</button>
          <button type="button" className="quiet" onClick={() => moveMatch(1)} disabled={matches.length === 0} aria-label="Siguiente coincidencia">↓</button>
          <button type="button" className="quiet" onClick={onFindClose} aria-label="Cerrar búsqueda">
            ×
          </button>
        </div>
      )}
      <textarea
        ref={editorRef}
        className="markdown-editor"
        aria-label="Editor Markdown"
        spellCheck
        value={content}
        onChange={(event) => onChange(event.target.value)}
      />
    </section>
  );
}
