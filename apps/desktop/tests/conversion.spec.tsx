import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "../src/App";

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
  return {
    selectOpenDocument: vi.fn().mockResolvedValue(null),
    selectSaveDocument: vi.fn().mockResolvedValue({ name: "report.md", writeToken: "output-token" }),
    selectConversionSource: vi.fn().mockResolvedValue({ name: "report.pdf", readToken: "source-token" }),
    openDocument: vi.fn().mockResolvedValue({ content: "# Converted" }),
    saveDocument: vi.fn(),
    convertDocument: vi.fn(),
    cancelConversion: vi.fn().mockResolvedValue(undefined),
    claimPrintJob: vi.fn().mockResolvedValue({
      id: "job-id",
      title: "Quarterly / Report?",
      sourceToken: "print-token",
      createdUnixMs: 1,
    }),
    finishPrintJob: vi.fn().mockResolvedValue(undefined),
    integrationStatus: vi.fn().mockResolvedValue({ pendingPrintJobIds: [] }),
    activateWindow: vi.fn().mockResolvedValue(undefined),
    openExternal: vi.fn(),
    onPrintJobRequested: vi.fn(async (handler: typeof printHandler) => {
      printHandler = handler;
      return () => undefined;
    }),
    onCloseRequested: vi.fn().mockResolvedValue(() => undefined),
    emitPrintJob: (id: string) => printHandler?.(id),
  };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("conversion behavior", () => {
  it("runs a direct local conversion, exposes progress and can cancel it", async () => {
    const conversion = deferred<never>();
    const api = backend();
    api.convertDocument.mockReturnValue(conversion.promise);
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Convertir archivo" }));
    expect(await screen.findByRole("progressbar", { name: "Conversión en curso" })).toBeInTheDocument();
    expect(api.convertDocument).toHaveBeenCalledWith({
      operationId: expect.any(String),
      sourceToken: "source-token",
      outputToken: "output-token",
    });

    fireEvent.click(screen.getByRole("button", { name: "Cancelar conversión" }));
    await waitFor(() => expect(api.cancelConversion).toHaveBeenCalledWith(expect.any(String)));
  });

  it("shows warnings and opens the converted Markdown in the same window", async () => {
    const api = backend();
    api.convertDocument.mockResolvedValue({
      operationId: "operation-id",
      markdownToken: "markdown-token",
      warningCodes: ["table_degraded", "missing_image_alt"],
    });
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Convertir archivo" }));
    expect(await screen.findByText("La tabla se simplificó durante la conversión.")).toBeInTheDocument();
    expect(screen.getByText("Falta texto alternativo en una imagen.")).toBeInTheDocument();
    expect(api.openDocument).toHaveBeenCalledWith("markdown-token");
    expect(screen.getByRole("textbox", { name: "Editor Markdown" })).toHaveValue("# Converted");
  });

  it("claims a print job, activates the current window and sanitizes the native Save As title", async () => {
    const api = backend();
    api.convertDocument.mockResolvedValue({
      operationId: "operation-id",
      markdownToken: "markdown-token",
      warningCodes: [],
    });
    render(<App backend={api} />);
    await waitFor(() => expect(api.onPrintJobRequested).toHaveBeenCalled());

    api.emitPrintJob("job-id");

    await waitFor(() => expect(api.activateWindow).toHaveBeenCalledTimes(1));
    expect(api.claimPrintJob).toHaveBeenCalledWith("job-id");
    expect(api.selectSaveDocument).toHaveBeenCalledWith("Quarterly Report.md");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-id"));
    expect(api.openDocument).toHaveBeenCalledWith("markdown-token");
  });

  it("finishes a claimed job when Save As is cancelled or conversion fails", async () => {
    const api = backend();
    api.selectSaveDocument.mockResolvedValueOnce(null).mockResolvedValueOnce({
      name: "report.md",
      writeToken: "output-token",
    });
    api.convertDocument.mockRejectedValueOnce(new Error("failed"));
    render(<App backend={api} />);
    await waitFor(() => expect(api.onPrintJobRequested).toHaveBeenCalled());

    api.emitPrintJob("first-job");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledWith("job-id"));
    api.emitPrintJob("second-job");
    await waitFor(() => expect(api.finishPrintJob).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo completar la conversión.");
  });
});
