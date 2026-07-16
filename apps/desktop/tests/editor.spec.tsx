import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "../src/App";

function backend() {
  let closeHandler: ((event: { preventDefault(): void }) => void) | undefined;
  return {
    selectOpenDocument: vi.fn().mockResolvedValue({
      name: "notes.md",
      readToken: "read-token",
      writeToken: "write-token",
    }),
    selectSaveDocument: vi.fn().mockResolvedValue({ name: "copy.md", writeToken: "copy-token" }),
    selectConversionSource: vi.fn().mockResolvedValue(null),
    openDocument: vi.fn().mockResolvedValue({ content: "# Original\n\nA [safe](https://example.com) link." }),
    saveDocument: vi.fn().mockResolvedValue({ saved: true }),
    convertDocument: vi.fn(),
    cancelConversion: vi.fn(),
    claimPrintJob: vi.fn(),
    finishPrintJob: vi.fn(),
    integrationStatus: vi.fn().mockResolvedValue({ pendingPrintJobIds: [] }),
    activateWindow: vi.fn(),
    openExternal: vi.fn(),
    onPrintJobRequested: vi.fn().mockResolvedValue(() => undefined),
    onCloseRequested: vi.fn(async (handler: typeof closeHandler) => {
      closeHandler = handler;
      return () => undefined;
    }),
    requestClose: () => closeHandler?.({ preventDefault: vi.fn() }),
  };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("viewer and editor behavior", () => {
  it("opens Markdown, edits it, reports dirty state and saves with its opaque capability", async () => {
    const api = backend();
    api.saveDocument.mockResolvedValueOnce({ saved: true, writeToken: "renewed-token" });
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Abrir Markdown" }));
    const editor = await screen.findByRole("textbox", { name: "Editor Markdown" });
    expect(editor).toHaveValue("# Original\n\nA [safe](https://example.com) link.");
    expect(screen.getByText("notes.md")).toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "# Changed" } });
    expect(screen.getByText("Cambios sin guardar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Guardar" }));

    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledWith("write-token", "# Changed"));
    expect(screen.getByText("Guardado")).toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "# Changed again" } });
    fireEvent.click(screen.getByRole("button", { name: "Guardar", exact: true }));
    await waitFor(() => expect(api.saveDocument).toHaveBeenLastCalledWith("renewed-token", "# Changed again"));
    expect(api.selectSaveDocument).not.toHaveBeenCalled();
  });

  it("supports Save As and keeps the new document in the current window", async () => {
    const api = backend();
    render(<App backend={api} />);

    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "New document" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Guardar como" }));

    await waitFor(() => expect(api.selectSaveDocument).toHaveBeenCalledWith("Sin título.md"));
    expect(api.saveDocument).toHaveBeenCalledWith("copy-token", "New document");
    expect(screen.getByText("copy.md")).toBeInTheDocument();
    expect(api.activateWindow).not.toHaveBeenCalled();
  });

  it("intercepts close when dirty and respects the user's confirmation", async () => {
    const api = backend();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "Unsaved" },
    });

    await waitFor(() => expect(api.onCloseRequested).toHaveBeenCalled());
    api.requestClose();
    expect(confirm).toHaveBeenCalledWith("Hay cambios sin guardar. ¿Cerrar de todos modos?");
  });

  it("opens find with the keyboard, counts matches and focuses the search field", async () => {
    const api = backend();
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "alpha beta alpha" },
    });

    fireEvent.keyDown(window, { key: "f", metaKey: true });
    const search = await screen.findByRole("searchbox", { name: "Buscar en el documento" });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "alpha" } });
    expect(screen.getByText("2 coincidencias")).toBeInTheDocument();
  });

  it("renders GFM without active HTML or unsafe links and confirms external navigation", async () => {
    const api = backend();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: {
        value:
          "# Preview\n\n- [x] done\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n<a onclick=\"alert(1)\">bad</a><script>alert(1)</script> [unsafe](javascript:alert(1)) [section](#preview) [outside](https://example.com)",
      },
    });

    const preview = screen.getByRole("region", { name: "Vista previa" });
    expect(within(preview).getByRole("heading", { name: "Preview" })).toBeInTheDocument();
    expect(within(preview).getByRole("checkbox", { name: "done" })).toBeChecked();
    expect(within(preview).getByRole("table")).toBeInTheDocument();
    expect(preview.querySelector("script")).toBeNull();
    expect(preview.querySelector("[onclick]")).toBeNull();
    expect(within(preview).queryByRole("link", { name: "unsafe" })).toBeNull();
    expect(within(preview).getByRole("link", { name: "section" })).toHaveAttribute("href", "#preview");

    fireEvent.click(within(preview).getByRole("link", { name: "outside" }));
    expect(confirm).toHaveBeenCalled();
    expect(api.openExternal).toHaveBeenCalledWith("https://example.com");
  });

  it("persists light, dark and system theme preference locally", () => {
    const api = backend();
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Tema" }), { target: { value: "dark" } });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("mdviewer.theme")).toBe("dark");
  });
});
