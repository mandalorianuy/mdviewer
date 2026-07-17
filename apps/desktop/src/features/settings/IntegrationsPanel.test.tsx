import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { IntegrationsPanel } from "./IntegrationsPanel";
import type { MacosIntegrationBackend } from "../../lib/tauri";

afterEach(cleanup);

function backend(status: "not_installed" | "installed" | "outdated" | "invalid") {
  return {
    macosWorkflowStatus: vi.fn().mockResolvedValue(status),
    installMacosWorkflow: vi.fn().mockResolvedValue("installed"),
    repairMacosWorkflow: vi.fn().mockResolvedValue("installed"),
    uninstallMacosWorkflow: vi.fn().mockResolvedValue("not_installed"),
    macosVirtualPrinterStatus: vi.fn().mockResolvedValue("not_installed"),
    installMacosVirtualPrinter: vi.fn().mockResolvedValue("installed"),
    repairMacosVirtualPrinter: vi.fn().mockResolvedValue("installed"),
    uninstallMacosVirtualPrinter: vi.fn().mockResolvedValue("not_installed"),
  } satisfies MacosIntegrationBackend;
}

describe("macOS PDF Workflow integration", () => {
  it("installs from the not installed state and announces the result", async () => {
    const api = backend("not_installed");
    render(<IntegrationsPanel backend={api} />);

    expect((await screen.findAllByText("no instalada"))[0]).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Instalar flujo PDF de macOS" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Flujo PDF instalado.");
    expect(api.installMacosWorkflow).toHaveBeenCalledOnce();
    expect(screen.getByText("instalada")).toBeInTheDocument();
  });

  it("repairs the outdated state", async () => {
    const api = backend("outdated");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("desactualizada")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reparar flujo PDF de macOS" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Flujo PDF reparado.");
    expect(api.repairMacosWorkflow).toHaveBeenCalledOnce();
  });

  it("preserves an invalid unrelated item and offers no destructive action", async () => {
    const api = backend("invalid");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("no verificable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reparar flujo PDF de macOS" })).not.toBeInTheDocument();
    expect(screen.getByText(/preservó/i)).toBeInTheDocument();
    expect(api.repairMacosWorkflow).not.toHaveBeenCalled();
  });

  it("requires an accessible confirmation before uninstalling", async () => {
    const api = backend("installed");
    render(<IntegrationsPanel backend={api} />);
    await screen.findByText("instalada");

    fireEvent.click(screen.getByRole("button", { name: "Desinstalar flujo PDF de macOS" }));
    const dialog = screen.getByRole("alertdialog", { name: "¿Desinstalar el flujo PDF de macOS?" });
    expect(dialog).toBeInTheDocument();
    expect(api.uninstallMacosWorkflow).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Confirmar desinstalación del flujo PDF" }));
    await waitFor(() => expect(api.uninstallMacosWorkflow).toHaveBeenCalledOnce());
    expect(await screen.findByRole("status")).toHaveTextContent("Flujo PDF desinstalado.");
  });

  it("announces lifecycle failures without changing the state", async () => {
    const api = backend("outdated");
    api.repairMacosWorkflow.mockRejectedValue({ code: "unsafe_workflow_target" });
    render(<IntegrationsPanel backend={api} />);
    await screen.findByText("desactualizada");

    fireEvent.click(screen.getByRole("button", { name: "Reparar flujo PDF de macOS" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo reparar el flujo PDF.");
    expect(screen.getByText("desactualizada")).toBeInTheDocument();
  });
});

describe("macOS virtual printer integration", () => {
  it("explains and installs the Chrome-compatible print destination", async () => {
    const api = backend("installed");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("Chrome y otras aplicaciones")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Instalar impresora virtual de macOS" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Impresora virtual instalada.");
    expect(api.installMacosVirtualPrinter).toHaveBeenCalledOnce();
  });

  it("requires confirmation before removing the Chrome-compatible destination", async () => {
    const api = backend("installed");
    api.macosVirtualPrinterStatus.mockResolvedValue("installed");
    render(<IntegrationsPanel backend={api} />);

    await screen.findByText("Chrome y otras aplicaciones");
    fireEvent.click(screen.getByRole("button", { name: "Desinstalar impresora virtual de macOS" }));

    expect(screen.getByRole("alertdialog", { name: "¿Desinstalar la impresora virtual de macOS?" })).toBeInTheDocument();
    expect(api.uninstallMacosVirtualPrinter).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Confirmar desinstalación de la impresora virtual" }));
    await waitFor(() => expect(api.uninstallMacosVirtualPrinter).toHaveBeenCalledOnce());
  });
});
