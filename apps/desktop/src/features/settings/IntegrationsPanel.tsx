import { useEffect, useState } from "react";

import type { MacosIntegrationBackend, MacosWorkflowStatus } from "../../lib/tauri";

interface IntegrationsPanelProps {
  backend: MacosIntegrationBackend;
  onClose?: () => void;
}

const labels: Record<MacosWorkflowStatus, string> = {
  not_installed: "not installed",
  installed: "installed",
  outdated: "outdated",
  invalid: "invalid",
};

export function IntegrationsPanel({ backend, onClose }: IntegrationsPanelProps) {
  const [status, setStatus] = useState<MacosWorkflowStatus>();
  const [busy, setBusy] = useState(false);
  const [confirmingUninstall, setConfirmingUninstall] = useState(false);
  const [result, setResult] = useState<string>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    let alive = true;
    void backend.macosWorkflowStatus()
      .then((value) => { if (alive) setStatus(value); })
      .catch(() => { if (alive) setError("The workflow status could not be checked."); });
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

  return (
    <section className="integrations-panel" aria-labelledby="integrations-heading">
      <div className="integrations-heading-row">
        <div>
          <h2 id="integrations-heading">Integrations</h2>
          <p>macOS PDF Workflow</p>
        </div>
        {onClose && <button type="button" className="quiet" aria-label="Close integrations" onClick={onClose}>Close</button>}
      </div>
      <div className="integration-row">
        <div>
          <strong>Guardar como Markdown con MDViewer</strong>
          <span className="integration-state">{status ? labels[status] : "checking"}</span>
        </div>
        {status === "not_installed" && (
          <button type="button" disabled={busy} aria-label="Install macOS PDF Workflow" onClick={() => void perform(
            () => backend.installMacosWorkflow(),
            "Workflow installed.",
            "The workflow could not be installed.",
          )}>Install</button>
        )}
        {(status === "outdated" || status === "invalid") && (
          <button type="button" disabled={busy} aria-label="Repair macOS PDF Workflow" onClick={() => void perform(
            () => backend.repairMacosWorkflow(),
            "Workflow repaired.",
            "The workflow could not be repaired.",
          )}>Repair</button>
        )}
        {status === "installed" && !confirmingUninstall && (
          <button type="button" disabled={busy} aria-label="Uninstall macOS PDF Workflow" onClick={() => setConfirmingUninstall(true)}>Uninstall</button>
        )}
      </div>
      {confirmingUninstall && (
        <div role="alertdialog" aria-labelledby="uninstall-heading" aria-describedby="uninstall-description" className="integration-confirmation">
          <strong id="uninstall-heading">Uninstall macOS PDF Workflow?</strong>
          <p id="uninstall-description">This removes only MDViewer’s PDF Services workflow.</p>
          <button type="button" className="quiet" onClick={() => setConfirmingUninstall(false)}>Cancel</button>
          <button type="button" aria-label="Confirm uninstall" onClick={() => void perform(
            () => backend.uninstallMacosWorkflow(),
            "Workflow uninstalled.",
            "The workflow could not be uninstalled.",
          )}>Uninstall</button>
        </div>
      )}
      {result && <p role="status" className="integration-result">{result}</p>}
      {error && <p role="alert" className="integration-error">{error}</p>}
    </section>
  );
}
