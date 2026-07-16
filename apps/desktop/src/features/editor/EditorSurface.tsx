import { useEffect, useRef } from "react";

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
  useEffect(() => {
    if (findOpen) searchRef.current?.focus();
  }, [findOpen]);

  const matches = findQuery
    ? content.toLocaleLowerCase().split(findQuery.toLocaleLowerCase()).length - 1
    : 0;

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
            {matches} {matches === 1 ? "coincidencia" : "coincidencias"}
          </span>
          <button type="button" className="quiet" onClick={onFindClose} aria-label="Cerrar búsqueda">
            ×
          </button>
        </div>
      )}
      <textarea
        className="markdown-editor"
        aria-label="Editor Markdown"
        spellCheck
        value={content}
        onChange={(event) => onChange(event.target.value)}
      />
    </section>
  );
}
