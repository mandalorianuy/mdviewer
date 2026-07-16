import { useCallback, useEffect, useRef, useState } from "react";

import { warningMessage } from "./features/conversion/warnings";
import {
  confirmReplacement,
  markdownName,
  transitionDocument,
  untitledDocument,
  type DocumentSnapshot,
  type DocumentState,
} from "./features/documents/document";
import { EditorSurface } from "./features/editor/EditorSurface";
import { MarkdownPreview } from "./features/preview/MarkdownPreview";
import { ThemeSelect, type ThemePreference } from "./features/settings/ThemeSelect";
import { IntegrationsPanel } from "./features/settings/IntegrationsPanel";
import {
  isBackendErrorCode,
  tauriBackend,
  type Backend,
  type ConversionResult,
} from "./lib/tauri";
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
  const macosIntegrationsAvailable = /Mac/.test(navigator.platform || navigator.userAgent);
  const [currentDocument, setDocument] = useState<DocumentState>(untitledDocument);
  const [theme, setTheme] = useState<ThemePreference>(initialTheme);
  const [findOpen, setFindOpen] = useState(false);
  const [integrationsOpen, setIntegrationsOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeOperation, setActiveOperation] = useState<string>();
  const [cancellingOperation, setCancellingOperation] = useState<string>();
  const [conversionNotice, setConversionNotice] = useState<string>();
  const [openBusy, setOpenBusy] = useState(false);
  const [saveBusy, setSaveBusy] = useState(false);
  const [workflowBusy, setWorkflowBusy] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string>();
  const [jobCleanupErrors, setJobCleanupErrors] = useState<Record<string, string>>({});
  const dirty = currentDocument.content !== currentDocument.savedContent;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const documentRef = useRef(currentDocument);
  documentRef.current = currentDocument;
  const documentGeneration = useRef(1);
  const openBusyRef = useRef(false);
  const inFlightJobs = useRef(new Set<string>());
  const pendingJobFinishes = useRef(new Set<string>());
  const terminalJobs = useRef(new Set<string>());
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

  const replaceDocument = useCallback((
    name: string,
    content: string,
    writeToken?: string,
    transitionType: "replace" | "conversion-completed" = "replace",
  ) => {
    const replacement = {
      generation: documentGeneration.current,
      name,
      content,
      savedContent: content,
      writeToken,
    };
    documentGeneration.current += 1;
    const transition = transitionType === "conversion-completed"
      ? { type: "conversion-completed" as const, accepted: true as const, replacement }
      : { type: "replace" as const, replacement };
    documentRef.current = transitionDocument(documentRef.current, transition);
    setDocument((current) => transitionDocument(current, transition));
  }, []);

  const openFromToken = useCallback(async (token: string, name: string, writeToken?: string) => {
    const opened = await backend.openDocument(token);
    replaceDocument(name, opened.content, writeToken);
  }, [backend, replaceDocument]);

  const openMarkdown = async () => {
    if (openBusyRef.current) return;
    openBusyRef.current = true;
    setOpenBusy(true);
    try {
      if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Abrir otro documento?")) return;
      const selection = await backend.selectOpenDocument();
      if (selection) await openFromToken(selection.readToken, selection.name, selection.writeToken);
    } finally {
      openBusyRef.current = false;
      setOpenBusy(false);
    }
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
    const submittedDocument = documentRef.current;
    const selection = await backend.selectSaveDocument(submittedDocument.name);
    if (!selection) return false;
    const submittedContent = submittedDocument.content;
    const saved = await backend.saveDocument(selection.writeToken, submittedContent);
    setDocument((current) => transitionDocument(current, {
      type: "save-completed",
      completion: {
        generation: submittedDocument.generation,
        name: selection.name,
        savedContent: submittedContent,
        writeToken: saved.writeToken,
      },
    }));
    return true;
  }, [backend]);

  const saveAs = useCallback(
    () => withSaveLock(saveAsUnlocked),
    [saveAsUnlocked, withSaveLock],
  );

  const save = useCallback(() => withSaveLock(async () => {
    const submittedDocument = documentRef.current;
    if (!submittedDocument.writeToken) return saveAsUnlocked();
    const submittedContent = submittedDocument.content;
    const saved = await backend.saveDocument(submittedDocument.writeToken, submittedContent);
    setDocument((current) => transitionDocument(current, {
      type: "save-completed",
      completion: {
        generation: submittedDocument.generation,
        savedContent: submittedContent,
        writeToken: saved.writeToken,
      },
    }));
    return true;
  }), [backend, saveAsUnlocked, withSaveLock]);

  const convert = useCallback(async (
    sourceToken: string,
    outputToken: string,
    outputName: string,
    replacementSnapshot: DocumentSnapshot,
  ) => {
    const id = operationId();
    setError(undefined);
    setConversionNotice(undefined);
    setWarnings([]);
    setActiveOperation(id);
    try {
      const result = await backend.convertDocument({ operationId: id, sourceToken, outputToken });
      const opened = await backend.openDocument(result.markdownToken);
      const accepted = confirmReplacement(
        documentRef.current,
        replacementSnapshot,
        dirtyRef.current,
        (message) => window.confirm(message),
      );
      if (!accepted) {
        setDocument((current) => transitionDocument(current, {
          type: "conversion-completed",
          accepted: false,
        }));
        return false;
      }
      setWarnings(result.warningCodes);
      replaceDocument(outputName, opened.content, result.writeToken, "conversion-completed");
      return true;
    } catch (reason) {
      if (isBackendErrorCode(reason, "cancelled")) {
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
  }, [backend, replaceDocument]);

  const convertDirectly = useCallback(async () => {
    if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?")) return;
    const replacementSnapshot = {
      generation: documentRef.current.generation,
      content: documentRef.current.content,
    };
    const source = await backend.selectConversionSource();
    if (!source) return;
    const output = await backend.selectSaveDocument(markdownName(source.name.replace(/\.[^.]+$/, "")));
    if (!output) return;
    await convert(source.readToken, output.writeToken, output.name, replacementSnapshot);
  }, [backend, convert]);

  const clearJobCleanupError = useCallback((id: string) => {
    setJobCleanupErrors((current) => {
      if (!(id in current)) return current;
      const next = { ...current };
      delete next[id];
      return next;
    });
  }, []);

  const handlePrintJob = useCallback(async (id: string, finishOnly: boolean) => {
    let claimedId: string | undefined;
    if (finishOnly) {
      claimedId = id;
    } else {
      try {
        await backend.activateWindow();
        const job = await backend.claimPrintJob(id);
        claimedId = job.id;
        if (dirtyRef.current && !window.confirm("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?")) {
          // The durable terminal finish below still applies to a rejected replacement.
        } else {
          const replacementSnapshot = {
            generation: documentRef.current.generation,
            content: documentRef.current.content,
          };
          const output = await backend.selectSaveDocument(markdownName(job.title));
          if (output) {
            await convert(job.sourceToken, output.writeToken, output.name, replacementSnapshot);
          }
        }
      } catch {
        setError("No se pudo completar la conversión.");
      }
    }

    if (!claimedId) return "retryable" as const;
    try {
      await backend.finishPrintJob(claimedId);
      clearJobCleanupError(id);
      return "terminal" as const;
    } catch (reason) {
      if (finishOnly && isBackendErrorCode(reason, "job_not_found")) {
        clearJobCleanupError(id);
        return "terminal" as const;
      }
      setJobCleanupErrors((current) => ({
        ...current,
        [id]: "No se pudo finalizar el trabajo de impresión. Volvé a intentarlo.",
      }));
      return "finish-pending" as const;
    }
  }, [backend, clearJobCleanupError, convert]);

  const queuePrintJob = useCallback((id: string) => {
    if (terminalJobs.current.has(id) || inFlightJobs.current.has(id)) return;
    const finishOnly = pendingJobFinishes.current.has(id);
    inFlightJobs.current.add(id);
    enqueueWorkflow(async () => {
      const result = await handlePrintJob(id, finishOnly);
      inFlightJobs.current.delete(id);
      if (result === "terminal") {
        pendingJobFinishes.current.delete(id);
        terminalJobs.current.add(id);
      } else if (result === "finish-pending") {
        pendingJobFinishes.current.add(id);
      }
    });
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
        <button type="button" onClick={() => void openMarkdown()} disabled={openBusy || saveBusy || workflowBusy}>Abrir Markdown</button>
        <button type="button" onClick={() => void save()} disabled={!dirty || openBusy || saveBusy || workflowBusy}>Guardar</button>
        <button type="button" className="quiet" onClick={() => void saveAs()} disabled={openBusy || saveBusy || workflowBusy}>Guardar como</button>
        <span className="toolbar-separator" />
        <button type="button" className="quiet" onClick={() => setFindOpen(true)}>Buscar</button>
        {macosIntegrationsAvailable && <button type="button" className="quiet" onClick={() => setIntegrationsOpen(true)}>Integrations</button>}
        <button
          type="button"
          className="accent"
          disabled={openBusy || saveBusy || workflowBusy}
          onClick={() => enqueueWorkflow(convertDirectly)}
        >
          Convertir archivo
        </button>
      </nav>
      {integrationsOpen && <IntegrationsPanel backend={backend} onClose={() => setIntegrationsOpen(false)} />}
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
      {Object.entries(jobCleanupErrors).map(([id, message]) => (
        <div className="error-banner" role="alert" key={id}>
          <span>{message} Trabajo: {id}.</span>
          <button
            type="button"
            className="quiet"
            aria-label={`Reintentar finalización de ${id}`}
            onClick={() => queuePrintJob(id)}
          >
            Reintentar
          </button>
        </div>
      ))}
      {warnings.length > 0 && <aside className="warning-list" aria-label="Advertencias"><strong>Conversión completada con observaciones</strong><ul>{warnings.map((code, index) => <li key={`${code}-${index}`}>{warningMessage(code)}</li>)}</ul></aside>}
      <div className="workspace">
        <EditorSurface content={currentDocument.content} findOpen={findOpen} findQuery={findQuery} onChange={(content) => setDocument((current) => ({ ...current, content }))} onFindChange={setFindQuery} onFindClose={() => setFindOpen(false)} />
        <MarkdownPreview markdown={currentDocument.content} onExternalLink={followExternal} />
      </div>
      <footer className="status-bar"><span>{currentDocument.content.length} caracteres</span><span>Procesamiento local</span></footer>
    </main>
  );
}
