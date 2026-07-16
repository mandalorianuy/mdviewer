import { useCallback, useEffect, useRef, useState } from "react";

import { warningMessage } from "./features/conversion/warnings";
import { markdownName, untitledDocument, type DocumentState } from "./features/documents/document";
import { EditorSurface } from "./features/editor/EditorSurface";
import { MarkdownPreview } from "./features/preview/MarkdownPreview";
import { ThemeSelect, type ThemePreference } from "./features/settings/ThemeSelect";
import { tauriBackend, type Backend, type ConversionResult } from "./lib/tauri";
import "./styles/app.css";

interface AppProps {
  backend?: Backend;
}

function operationId(): string {
  return globalThis.crypto?.randomUUID?.() ?? `00000000-0000-4000-8000-${Date.now().toString().padStart(12, "0").slice(-12)}`;
}

function initialTheme(): ThemePreference {
  const stored = window.localStorage.getItem("mdviewer.theme");
  return stored === "light" || stored === "dark" ? stored : "system";
}

export default function App({ backend = tauriBackend }: AppProps) {
  const [currentDocument, setDocument] = useState<DocumentState>(untitledDocument);
  const [theme, setTheme] = useState<ThemePreference>(initialTheme);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeOperation, setActiveOperation] = useState<string>();
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const dirty = currentDocument.content !== currentDocument.savedContent;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const handledJobs = useRef(new Set<string>());

  useEffect(() => {
    window.document.documentElement.dataset.theme = theme;
    window.localStorage.setItem("mdviewer.theme", theme);
  }, [theme]);

  useEffect(() => {
    const find = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        setFindOpen(true);
      }
    };
    window.addEventListener("keydown", find);
    return () => window.removeEventListener("keydown", find);
  }, []);

  useEffect(() => {
    let alive = true;
    let unlisten: () => void = () => undefined;
    void backend.onCloseRequested((event) => {
      if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Cerrar de todos modos?")) {
        event.preventDefault();
      }
    }).then((remove) => { if (alive) unlisten = remove; else remove(); }).catch(() => undefined);
    return () => { alive = false; unlisten(); };
  }, [backend]);

  const openFromToken = useCallback(async (token: string, name: string, writeToken?: string) => {
    const opened = await backend.openDocument(token);
    setDocument({ name, content: opened.content, savedContent: opened.content, writeToken });
  }, [backend]);

  const openMarkdown = async () => {
    if (dirty && !window.confirm("Hay cambios sin guardar. ¿Abrir otro documento?")) return;
    const selection = await backend.selectOpenDocument();
    if (selection) await openFromToken(selection.readToken, selection.name, selection.writeToken);
  };

  const saveAs = useCallback(async () => {
    const selection = await backend.selectSaveDocument(currentDocument.name);
    if (!selection) return false;
    const saved = await backend.saveDocument(selection.writeToken, currentDocument.content);
    setDocument((current) => ({ ...current, name: selection.name, savedContent: current.content, writeToken: saved.writeToken }));
    return true;
  }, [backend, currentDocument.content, currentDocument.name]);

  const save = async () => {
    if (!currentDocument.writeToken) {
      await saveAs();
      return;
    }
    const saved = await backend.saveDocument(currentDocument.writeToken, currentDocument.content);
    setDocument((current) => ({ ...current, savedContent: current.content, writeToken: saved.writeToken }));
  };

  const finishConversion = useCallback(async (result: ConversionResult, name: string) => {
    setWarnings(result.warningCodes);
    await openFromToken(result.markdownToken, name, result.writeToken);
  }, [openFromToken]);

  const convert = useCallback(async (sourceToken: string, outputToken: string, outputName: string) => {
    const id = operationId();
    setError(undefined);
    setWarnings([]);
    setActiveOperation(id);
    try {
      const result = await backend.convertDocument({ operationId: id, sourceToken, outputToken });
      await finishConversion(result, outputName);
      return true;
    } catch {
      setError("No se pudo completar la conversión.");
      return false;
    } finally {
      setActiveOperation(undefined);
    }
  }, [backend, finishConversion]);

  const convertDirectly = async () => {
    const source = await backend.selectConversionSource();
    if (!source) return;
    const output = await backend.selectSaveDocument(markdownName(source.name.replace(/\.[^.]+$/, "")));
    if (!output) return;
    await convert(source.readToken, output.writeToken, output.name);
  };

  const handlePrintJob = useCallback(async (id: string) => {
    if (handledJobs.current.has(id)) return;
    handledJobs.current.add(id);
    let claimedId: string | undefined;
    try {
      await backend.activateWindow();
      const job = await backend.claimPrintJob(id);
      claimedId = job.id;
      const output = await backend.selectSaveDocument(markdownName(job.title));
      if (!output) return;
      await convert(job.sourceToken, output.writeToken, output.name);
    } catch {
      setError("No se pudo completar la conversión.");
    } finally {
      if (claimedId) await backend.finishPrintJob(claimedId).catch(() => undefined);
    }
  }, [backend, convert]);

  useEffect(() => {
    let alive = true;
    let unlisten: () => void = () => undefined;
    void backend.onPrintJobRequested((id) => void handlePrintJob(id))
      .then((remove) => { if (alive) unlisten = remove; else remove(); })
      .catch(() => undefined);
    void backend.integrationStatus()
      .then((status) => status.pendingPrintJobIds.forEach((id) => void handlePrintJob(id)))
      .catch(() => undefined);
    return () => { alive = false; unlisten(); };
  }, [backend, handlePrintJob]);

  const followExternal = (url: string) => {
    if (window.confirm(`Abrir este enlace en el navegador del sistema?\n\n${url}`)) {
      void Promise.resolve(backend.openExternal(url)).catch(() => setError("No se pudo abrir el enlace externo."));
    }
  };

  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="brand"><span className="brand-mark" aria-hidden="true">M↓</span><div><h1>MDViewer</h1><p>Markdown local, sin salir de tu equipo</p></div></div>
        <ThemeSelect value={theme} onChange={setTheme} />
      </header>
      <nav className="toolbar" aria-label="Documento">
        <button type="button" onClick={() => void openMarkdown()}>Abrir Markdown</button>
        <button type="button" onClick={() => void save()} disabled={!dirty}>Guardar</button>
        <button type="button" className="quiet" onClick={() => void saveAs()}>Guardar como</button>
        <span className="toolbar-separator" />
        <button type="button" className="quiet" onClick={() => setFindOpen(true)}>Buscar</button>
        <button type="button" className="accent" onClick={() => void convertDirectly()}>Convertir archivo</button>
      </nav>
      <section className="document-bar" aria-label="Estado del documento">
        <strong>{currentDocument.name}</strong>
        <span className={dirty ? "dirty" : "saved"}>{dirty ? "Cambios sin guardar" : "Guardado"}</span>
      </section>
      {activeOperation && (
        <section className="conversion-status" aria-live="polite">
          <progress aria-label="Conversión en curso" />
          <span>Convirtiendo localmente…</span>
          <button type="button" className="quiet" onClick={() => void backend.cancelConversion(activeOperation)}>Cancelar conversión</button>
        </section>
      )}
      {error && <div className="error-banner" role="alert">{error}</div>}
      {warnings.length > 0 && <aside className="warning-list" aria-label="Advertencias"><strong>Conversión completada con observaciones</strong><ul>{warnings.map((code, index) => <li key={`${code}-${index}`}>{warningMessage(code)}</li>)}</ul></aside>}
      <div className="workspace">
        <EditorSurface content={currentDocument.content} findOpen={findOpen} findQuery={findQuery} onChange={(content) => setDocument((current) => ({ ...current, content }))} onFindChange={setFindQuery} onFindClose={() => setFindOpen(false)} />
        <MarkdownPreview markdown={currentDocument.content} onExternalLink={followExternal} />
      </div>
      <footer className="status-bar"><span>{currentDocument.content.length} caracteres</span><span>Procesamiento local</span></footer>
    </main>
  );
}
