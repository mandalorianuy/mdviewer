import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "../src/App";
import type { Backend } from "../src/lib/tauri";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function backend() {
  let printHandler: ((id: string) => void) | undefined;
  const api = {
    selectOpenDocument: vi.fn().mockResolvedValue(null),
    selectSaveDocument: vi.fn().mockResolvedValue({ name: "report.md", writeToken: "output-token" }),
    selectConversionSource: vi.fn().mockResolvedValue({ name: "report.pdf", readToken: "source-token" }),
    openDocument: vi.fn().mockResolvedValue({ content: "# Converted" }),
    saveDocument: vi.fn().mockResolvedValue({ saved: true, writeToken: "write-token" }),
    convertDocument: vi.fn(),
    cancelConversion: vi.fn().mockResolvedValue(undefined),
    claimPrintJob: vi.fn().mockImplementation(async (id: string) => ({
      id,
      title: id === "job-id" ? "Quarterly / Report?" : `Report ${id}`,
      sourceToken: `print-token-${id}`,
      createdUnixMs: 1,
    })),
    finishPrintJob: vi.fn().mockResolvedValue(undefined),
    integrationStatus: vi.fn().mockResolvedValue({ pendingPrintJobIds: [] }),
    activateWindow: vi.fn().mockResolvedValue(undefined),
    openExternal: vi.fn().mockResolvedValue(undefined),
    onPrintJobRequested: vi.fn(async (handler: (id: string) => void) => {
      printHandler = handler;
      return () => undefined;
    }),
    onCloseRequested: vi.fn().mockResolvedValue(() => undefined),
    emitPrintJob: (id: string) => printHandler?.(id),
  } satisfies Backend & { emitPrintJob(id: string): void };
  return api;
}

const converted = {
  operationId: "operation-id",
  markdownToken: "markdown-token",
  writeToken: "converted-write-token",
  warningCodes: [] as string[],
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("conversion behavior", () => {
  it("shows cancelling state and distinguishes typed cancellation from failure", async () => {
    const conversion = deferred<typeof converted>();
    const cancellation = deferred<void>();
    const api = backend();
    api.convertDocument.mockReturnValue(conversion.promise);
    api.cancelConversion.mockReturnValue(cancellation.promise);
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Convertir archivo" }));
    expect(await screen.findByRole("progressbar", { name: "Conversión en curso" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancelar conversión" }));
    const cancelling = screen.getByRole("button", { name: "Cancelando conversión" });
    expect(cancelling).toBeDisabled();
    expect(api.cancelConversion).toHaveBeenCalledWith(expect.any(String));

    await act(async () => {
      cancellation.resolve();
      conversion.reject({ code: "cancelled", message: "conversion was cancelled" });
      await Promise.resolve();
    });
    expect(await screen.findByText("Conversión cancelada.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("progressbar")).toBeNull();
  });

  it("does not replace a dirty editor when conversion is rejected", async () => {
    const api = backend();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<App backend={api} />);
    const editor = screen.getByRole("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, { target: { value: "unsaved draft" } });

    fireEvent.click(screen.getByRole("button", { name: "Convertir archivo" }));

    expect(confirm).toHaveBeenCalledWith("Hay cambios sin guardar. ¿Reemplazar el documento con la conversión?");
    expect(api.selectConversionSource).not.toHaveBeenCalled();
    expect(api.convertDocument).not.toHaveBeenCalled();
    expect(editor).toHaveValue("unsaved draft");
  });

  it("replaces a dirty editor after confirmation and opens warnings in the same window", async () => {
    const api = backend();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    api.convertDocument.mockResolvedValue({
      ...converted,
      warningCodes: ["table_degraded", "missing_image_alt"],
    });
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), { target: { value: "dirty" } });

    fireEvent.click(screen.getByRole("button", { name: "Convertir archivo" }));
    expect(await screen.findByText("La tabla se simplificó durante la conversión.")).toBeInTheDocument();
    expect(screen.getByText("Falta texto alternativo en una imagen.")).toBeInTheDocument();
    expect(api.openDocument).toHaveBeenCalledWith("markdown-token");
    expect(screen.getByRole("textbox", { name: "Editor Markdown" })).toHaveValue("# Converted");
  });

  it("finishes a claimed print job when dirty replacement is rejected", async () => {
    const api = backend();
    vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), { target: { value: "dirty" } });
    await waitFor(() => expect(api.onPrintJobRequested).toHaveBeenCalled());

    api.emitPrintJob("job-id");

    await waitFor(() => expect(api.claimPrintJob).toHaveBeenCalledWith("job-id"));
    expect(api.selectSaveDocument).not.toHaveBeenCalled();
    expect(api.convertDocument).not.toHaveBeenCalled();
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-id"));
  });

  it("claims a print job, activates the current window and sanitizes the Save As title", async () => {
    const api = backend();
    api.convertDocument.mockResolvedValue(converted);
    render(<App backend={api} />);
    await waitFor(() => expect(api.onPrintJobRequested).toHaveBeenCalled());

    api.emitPrintJob("job-id");

    await waitFor(() => expect(api.activateWindow).toHaveBeenCalledTimes(1));
    expect(api.claimPrintJob).toHaveBeenCalledWith("job-id");
    expect(api.selectSaveDocument).toHaveBeenCalledWith("Quarterly Report.md");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-id"));
    expect(api.openDocument).toHaveBeenCalledWith("markdown-token");
  });

  it("serializes pending claimed jobs and disables manual conversion while a dialog is pending", async () => {
    const firstDialog = deferred<{ name: string; writeToken: string } | null>();
    const api = backend();
    api.integrationStatus.mockResolvedValue({ pendingPrintJobIds: ["job-1", "job-2"] });
    api.selectSaveDocument
      .mockReturnValueOnce(firstDialog.promise)
      .mockResolvedValueOnce(null);
    render(<App backend={api} />);

    await waitFor(() => expect(api.claimPrintJob).toHaveBeenCalledWith("job-1"));
    expect(api.claimPrintJob).not.toHaveBeenCalledWith("job-2");
    const manual = screen.getByRole("button", { name: "Convertir archivo" });
    expect(manual).toBeDisabled();
    fireEvent.click(manual);
    expect(api.selectConversionSource).not.toHaveBeenCalled();

    await act(() => firstDialog.resolve(null));
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-1"));
    await waitFor(() => expect(api.claimPrintJob).toHaveBeenCalledWith("job-2"));
    expect(api.convertDocument).not.toHaveBeenCalled();
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-2"));
    expect(api.claimPrintJob.mock.invocationCallOrder[0]).toBeLessThan(api.claimPrintJob.mock.invocationCallOrder[1]);
  });

  it("finishes claimed jobs when Save As is cancelled or conversion fails", async () => {
    const api = backend();
    api.selectSaveDocument.mockResolvedValueOnce(null).mockResolvedValueOnce({
      name: "report.md",
      writeToken: "output-token",
    });
    api.convertDocument.mockRejectedValueOnce(new Error("failed"));
    render(<App backend={api} />);
    await waitFor(() => expect(api.onPrintJobRequested).toHaveBeenCalled());

    api.emitPrintJob("first-job");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("first-job"));
    api.emitPrintJob("second-job");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("second-job"));
    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo completar la conversión.");
  });
});
