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
  const [cancellingOperation, setCancellingOperation] = useState<string>();
  const [conversionNotice, setConversionNotice] = useState<string>();
  const [saveBusy, setSaveBusy] = useState(false);
  const [workflowBusy, setWorkflowBusy] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const dirty = currentDocument.content !== currentDocument.savedContent;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const documentRef = useRef(currentDocument);
  documentRef.current = currentDocument;
  const handledJobs = useRef(new Set<string>());
  const saveBusyRef = useRef(false);
  const workflowTail = useRef<Promise<void>>(Promise.resolve());
  const workflowCount = useRef(0);

  const enqueueWorkflow = useCallback((task: () => Promise<void>) => {
    const startsImmediately = workflowCount.current === 0;
    workflowCount.current += 1;
    setWorkflowBusy(true);
    const queued = startsImmediately ? task() : workflowTail.current.then(task, task);
    workflowTail.current = queued.then(
      () => undefined,
      () => undefined,
    );
    void queued.finally(() => {
      workflowCount.current -= 1;
      setWorkflowBusy(workflowCount.current > 0);
    });
  }, []);

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

  const withSaveLock = useCallback(async <T,>(task: () => Promise<T>): Promise<T | undefined> => {
    if (saveBusyRef.current) return undefined;
    saveBusyRef.current = true;
    setSaveBusy(true);
    try {
      return await task();
    } finally {
      saveBusyRef.current = false;
      setSaveBusy(false);
    }
  }, []);

  const saveAsUnlocked = useCallback(async () => {
    const selection = await backend.selectSaveDocument(currentDocument.name);
    if (!selection) return false;
    const submittedContent = currentDocument.content;
    const saved = await backend.saveDocument(selection.writeToken, submittedContent);
    setDocument((current) => ({ ...current, name: selection.name, savedContent: submittedContent, writeToken: saved.writeToken }));
    return true;
  }, [backend, currentDocument.content, currentDocument.name]);

  const saveAs = useCallback(
    () => withSaveLock(saveAsUnlocked),
    [saveAsUnlocked, withSaveLock],
  );

  const save = useCallback(() => withSaveLock(async () => {
    if (!currentDocument.writeToken) return saveAsUnlocked();
    const submittedContent = currentDocument.content;
    const saved = await backend.saveDocument(currentDocument.writeToken, submittedContent);
    setDocument((current) => ({ ...current, savedContent: submittedContent, writeToken: saved.writeToken }));
    return true;
  }), [backend, currentDocument.content, currentDocument.writeToken, saveAsUnlocked, withSaveLock]);

  const finishConversion = useCallback(async (result: ConversionResult, name: string) => {
    setWarnings(result.warningCodes);
    await openFromToken(result.markdownToken, name, result.writeToken);
  }, [openFromToken]);

  const convert = useCallback(async (
    sourceToken: string,
    outputToken: string,
    outputName: string,
    replacementSnapshot: string,
  ) => {
    const id = operationId();
    setError(undefined);
    setConversionNotice(undefined);
    setWarnings([]);
    setActiveOperation(id);
    try {
      const result = await backend.convertDocument({ operationId: id, sourceToken, outputToken });
      if (
        dirtyRef.current
        && documentRef.current.content !== replacementSnapshot
        && !window.confirm("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?")
      ) {
        return false;
      }
      await finishConversion(result, outputName);
      return true;
    } catch (reason) {
      if (
        typeof reason === "object"
        && reason !== null
        && "code" in reason
        && reason.code === "cancelled"
      ) {
        setConversionNotice("Conversión cancelada.");
        setError(undefined);
      } else {
        setError("No se pudo completar la conversión.");
      }
      return false;
    } finally {
      setActiveOperation(undefined);
      setCancellingOperation(undefined);
    }
  }, [backend, finishConversion]);

  const convertDirectly = useCallback(async () => {
    if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?")) return;
    const replacementSnapshot = documentRef.current.content;
    const source = await backend.selectConversionSource();
    if (!source) return;
    const output = await backend.selectSaveDocument(markdownName(source.name.replace(/\.[^.]+$/, "")));
    if (!output) return;
    await convert(source.readToken, output.writeToken, output.name, replacementSnapshot);
  }, [backend, convert]);

  const handlePrintJob = useCallback(async (id: string) => {
    let claimedId: string | undefined;
    try {
      await backend.activateWindow();
      const job = await backend.claimPrintJob(id);
      claimedId = job.id;
      if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?")) return;
      const replacementSnapshot = documentRef.current.content;
      const output = await backend.selectSaveDocument(markdownName(job.title));
      if (!output) return;
      await convert(job.sourceToken, output.writeToken, output.name, replacementSnapshot);
    } catch {
      setError("No se pudo completar la conversión.");
    } finally {
      if (claimedId) await backend.finishPrintJob(claimedId).catch(() => undefined);
    }
  }, [backend, convert]);

  const queuePrintJob = useCallback((id: string) => {
    if (handledJobs.current.has(id)) return;
    handledJobs.current.add(id);
    enqueueWorkflow(() => handlePrintJob(id));
  }, [enqueueWorkflow, handlePrintJob]);

  useEffect(() => {
    let alive = true;
    let unlisten: () => void = () => undefined;
    void backend.onPrintJobRequested(queuePrintJob)
      .then((remove) => { if (alive) unlisten = remove; else remove(); })
      .catch(() => undefined);
    void backend.integrationStatus()
      .then((status) => status.pendingPrintJobIds.forEach(queuePrintJob))
      .catch(() => undefined);
    return () => { alive = false; unlisten(); };
  }, [backend, queuePrintJob]);

  const requestCancel = async (id: string) => {
    setCancellingOperation(id);
    try {
      await backend.cancelConversion(id);
    } catch {
      setCancellingOperation(undefined);
      setError("No se pudo cancelar la conversión.");
    }
  };

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
        <button type="button" onClick={() => void save()} disabled={!dirty || saveBusy}>Guardar</button>
        <button type="button" className="quiet" onClick={() => void saveAs()} disabled={saveBusy}>Guardar como</button>
        <span className="toolbar-separator" />
        <button type="button" className="quiet" onClick={() => setFindOpen(true)}>Buscar</button>
        <button
          type="button"
          className="accent"
          disabled={workflowBusy}
          onClick={() => enqueueWorkflow(convertDirectly)}
        >
          Convertir archivo
        </button>
      </nav>
      <section className="document-bar" aria-label="Estado del documento">
        <strong>{currentDocument.name}</strong>
        <span className={dirty ? "dirty" : "saved"}>{dirty ? "Cambios sin guardar" : "Guardado"}</span>
      </section>
      {activeOperation && (
        <section className="conversion-status" aria-live="polite">
          <progress aria-label="Conversión en curso" />
          <span>Convirtiendo localmente…</span>
          <button
            type="button"
            className="quiet"
            disabled={cancellingOperation === activeOperation}
            aria-label={cancellingOperation === activeOperation ? "Cancelando conversión" : "Cancelar conversión"}
            onClick={() => void requestCancel(activeOperation)}
          >
            {cancellingOperation === activeOperation ? "Cancelando conversión…" : "Cancelar conversión"}
          </button>
        </section>
      )}
      {conversionNotice && <div className="conversion-notice" role="status">{conversionNotice}</div>}
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
