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
  } satisfies MacosIntegrationBackend;
}

describe("macOS PDF Workflow integration", () => {
  it("installs from the not installed state and announces the result", async () => {
    const api = backend("not_installed");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("not installed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install macOS PDF Workflow" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Workflow installed.");
    expect(api.installMacosWorkflow).toHaveBeenCalledOnce();
    expect(screen.getByText("installed")).toBeInTheDocument();
  });

  it("repairs the outdated state", async () => {
    const api = backend("outdated");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("outdated")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Repair macOS PDF Workflow" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Workflow repaired.");
    expect(api.repairMacosWorkflow).toHaveBeenCalledOnce();
  });

  it("preserves an invalid unrelated item and offers no destructive action", async () => {
    const api = backend("invalid");
    render(<IntegrationsPanel backend={api} />);

    expect(await screen.findByText("invalid")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Repair macOS PDF Workflow" })).not.toBeInTheDocument();
    expect(screen.getByText(/preserved/i)).toBeInTheDocument();
    expect(api.repairMacosWorkflow).not.toHaveBeenCalled();
  });

  it("requires an accessible confirmation before uninstalling", async () => {
    const api = backend("installed");
    render(<IntegrationsPanel backend={api} />);
    await screen.findByText("installed");

    fireEvent.click(screen.getByRole("button", { name: "Uninstall macOS PDF Workflow" }));
    const dialog = screen.getByRole("alertdialog", { name: "Uninstall macOS PDF Workflow?" });
    expect(dialog).toBeInTheDocument();
    expect(api.uninstallMacosWorkflow).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Confirm uninstall" }));
    await waitFor(() => expect(api.uninstallMacosWorkflow).toHaveBeenCalledOnce());
    expect(await screen.findByRole("status")).toHaveTextContent("Workflow uninstalled.");
  });

  it("announces lifecycle failures without changing the state", async () => {
    const api = backend("outdated");
    api.repairMacosWorkflow.mockRejectedValue({ code: "unsafe_workflow_target" });
    render(<IntegrationsPanel backend={api} />);
    await screen.findByText("outdated");

    fireEvent.click(screen.getByRole("button", { name: "Repair macOS PDF Workflow" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("The workflow could not be repaired.");
    expect(screen.getByText("outdated")).toBeInTheDocument();
  });
});
