import { useEffect, useState } from "react";

import type { MacosIntegrationBackend, MacosWorkflowStatus } from "../../lib/tauri";

interface IntegrationsPanelProps {
  backend: MacosIntegrationBackend;
  onClose?: () => void;
}

const labels: Record<MacosWorkflowStatus, string> = {
  not_installed: "no instalada",
  installed: "instalada",
  outdated: "desactualizada",
  invalid: "no verificable",
};

export function IntegrationsPanel({ backend, onClose }: IntegrationsPanelProps) {
  const [status, setStatus] = useState<MacosWorkflowStatus>();
  const [virtualPrinterStatus, setVirtualPrinterStatus] = useState<MacosWorkflowStatus>();
  const [busy, setBusy] = useState(false);
  const [confirmingUninstall, setConfirmingUninstall] = useState(false);
  const [confirmingPrinterUninstall, setConfirmingPrinterUninstall] = useState(false);
  const [result, setResult] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    let alive = true;
    void backend.macosWorkflowStatus()
      .then((value) => { if (alive) setStatus(value); })
      .catch(() => { if (alive) setError("No se pudo comprobar el flujo PDF."); });
    void backend.macosVirtualPrinterStatus()
      .then((value) => { if (alive) setVirtualPrinterStatus(value); })
      .catch(() => { if (alive) setError("No se pudo comprobar la impresora virtual."); });
    return () => { alive = false; };
  }, [backend]);

  const perform = async (
    operation: () => Promise<MacosWorkflowStatus>,
    success: string,
    failure: string,
  ) => {
    setBusy(true);
    setResult(undefined);
    setError(undefined);
    try {
      setStatus(await operation());
      setResult(success);
    } catch {
      setError(failure);
    } finally {
      setBusy(false);
      setConfirmingUninstall(false);
    }
  };

  const performPrinter = async (
    operation: () => Promise<MacosWorkflowStatus>,
    success: string,
    failure: string,
  ) => {
    setBusy(true);
    setResult(undefined);
    setError(undefined);
    try {
      setVirtualPrinterStatus(await operation());
      setResult(success);
    } catch {
      setError(failure);
    } finally {
      setBusy(false);
      setConfirmingPrinterUninstall(false);
    }
  };

  return (
    <section className="integrations-panel" aria-labelledby="integrations-heading">
      <div className="integrations-heading-row">
        <div>
          <h2 id="integrations-heading">Integraciones</h2>
          <p>Impresión en macOS</p>
        </div>
        {onClose && <button type="button" className="quiet" aria-label="Cerrar integraciones" onClick={onClose}>Cerrar</button>}
      </div>
      <div className="integration-row">
        <div>
          <strong>Guardar como Markdown con MDViewer</strong>
          <span className="integration-description">Diálogo de impresión nativo</span>
          <span className="integration-state">{status ? labels[status] : "comprobando"}</span>
        </div>
        {status === "not_installed" && (
          <button type="button" disabled={busy} aria-label="Instalar flujo PDF de macOS" onClick={() => void perform(
            () => backend.installMacosWorkflow(),
            "Flujo PDF instalado.",
            "No se pudo instalar el flujo PDF.",
          )}>Instalar</button>
        )}
        {status === "outdated" && (
          <button type="button" disabled={busy} aria-label="Reparar flujo PDF de macOS" onClick={() => void perform(
            () => backend.repairMacosWorkflow(),
            "Flujo PDF reparado.",
            "No se pudo reparar el flujo PDF.",
          )}>Reparar</button>
        )}
        {status === "invalid" && (
          <p className="integration-warning">Se preservó un elemento ajeno o no verificable en PDF Services.</p>
        )}
        {status === "installed" && !confirmingUninstall && (
          <button type="button" disabled={busy} aria-label="Desinstalar flujo PDF de macOS" onClick={() => setConfirmingUninstall(true)}>Desinstalar</button>
        )}
      </div>
      <div className="integration-row">
        <div>
          <strong>MDViewer — Guardar como Markdown</strong>
          <span className="integration-description">Chrome y otras aplicaciones</span>
          <span className="integration-state">{virtualPrinterStatus ? labels[virtualPrinterStatus] : "comprobando"}</span>
        </div>
        {virtualPrinterStatus === "not_installed" && (
          <button type="button" disabled={busy} aria-label="Instalar impresora virtual de macOS" onClick={() => void performPrinter(
            () => backend.installMacosVirtualPrinter(),
            "Impresora virtual instalada.",
            "No se pudo instalar la impresora virtual.",
          )}>Instalar</button>
        )}
        {virtualPrinterStatus === "outdated" && (
          <button type="button" disabled={busy} aria-label="Reparar impresora virtual de macOS" onClick={() => void performPrinter(
            () => backend.repairMacosVirtualPrinter(),
            "Impresora virtual reparada.",
            "No se pudo reparar la impresora virtual.",
          )}>Reparar</button>
        )}
        {virtualPrinterStatus === "invalid" && (
          <p className="integration-warning">Se preservó una configuración de impresora ajena o no verificable.</p>
        )}
        {virtualPrinterStatus === "installed" && !confirmingPrinterUninstall && (
          <button type="button" disabled={busy} aria-label="Desinstalar impresora virtual de macOS" onClick={() => setConfirmingPrinterUninstall(true)}>Desinstalar</button>
        )}
      </div>
      {confirmingPrinterUninstall && (
        <div role="alertdialog" aria-labelledby="printer-uninstall-heading" aria-describedby="printer-uninstall-description" className="integration-confirmation">
          <strong id="printer-uninstall-heading">¿Desinstalar la impresora virtual de macOS?</strong>
          <p id="printer-uninstall-description">Se retirarán únicamente el destino CUPS y el LaunchAgent de MDViewer.</p>
          <button type="button" className="quiet" onClick={() => setConfirmingPrinterUninstall(false)}>Cancelar</button>
          <button type="button" aria-label="Confirmar desinstalación de la impresora virtual" onClick={() => void performPrinter(
            () => backend.uninstallMacosVirtualPrinter(),
            "Impresora virtual desinstalada.",
            "No se pudo desinstalar la impresora virtual.",
          )}>Desinstalar</button>
        </div>
      )}
      {confirmingUninstall && (
        <div role="alertdialog" aria-labelledby="uninstall-heading" aria-describedby="uninstall-description" className="integration-confirmation">
          <strong id="uninstall-heading">¿Desinstalar el flujo PDF de macOS?</strong>
          <p id="uninstall-description">Se retirará únicamente el flujo de PDF Services de MDViewer.</p>
          <button type="button" className="quiet" onClick={() => setConfirmingUninstall(false)}>Cancelar</button>
          <button type="button" aria-label="Confirmar desinstalación del flujo PDF" onClick={() => void perform(
            () => backend.uninstallMacosWorkflow(),
            "Flujo PDF desinstalado.",
            "No se pudo desinstalar el flujo PDF.",
          )}>Desinstalar</button>
        </div>
      )}
      {result && <p role="status" className="integration-result">{result}</p>}
      {error && <p role="alert" className="integration-error">{error}</p>}
    </section>
  );
}
