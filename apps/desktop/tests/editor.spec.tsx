import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "../src/App";
import {
  markdownName,
  transitionDocument,
  type DocumentState,
  type SaveCompletion,
} from "../src/features/documents/document";
import type { Backend, CloseRequestEvent } from "../src/lib/tauri";

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
  let closeHandler: ((event: CloseRequestEvent) => void) | undefined;
  const api = {
    selectOpenDocument: vi.fn().mockResolvedValue({
      name: "notes.md",
      readToken: "read-token",
      writeToken: "write-token",
    }),
    selectSaveDocument: vi.fn().mockResolvedValue({ name: "copy.md", writeToken: "copy-token" }),
    selectExportDocument: vi.fn().mockResolvedValue({ name: "copy.html", writeToken: "html-token" }),
    selectConversionSource: vi.fn().mockResolvedValue(null),
    openDocument: vi.fn().mockResolvedValue({ content: "# Original\n\nA [safe](https://example.com) link." }),
    saveDocument: vi.fn().mockResolvedValue({ saved: true, writeToken: "next-write-token" }),
    convertDocument: vi.fn(),
    cancelConversion: vi.fn().mockResolvedValue(undefined),
    claimPrintJob: vi.fn(),
    finishPrintJob: vi.fn().mockResolvedValue(undefined),
    integrationStatus: vi.fn().mockResolvedValue({ pendingPrintJobIds: [] }),
    macosWorkflowStatus: vi.fn().mockResolvedValue("not_installed" as const),
    installMacosWorkflow: vi.fn().mockResolvedValue("installed" as const),
    repairMacosWorkflow: vi.fn().mockResolvedValue("installed" as const),
    uninstallMacosWorkflow: vi.fn().mockResolvedValue("not_installed" as const),
    activateWindow: vi.fn().mockResolvedValue(undefined),
    openExternal: vi.fn().mockResolvedValue(undefined),
    onPrintJobRequested: vi.fn().mockResolvedValue(() => undefined),
    onCloseRequested: vi.fn(async (handler: (event: CloseRequestEvent) => void) => {
      closeHandler = handler;
      return () => undefined;
    }),
    requestClose: () => {
      const event = { preventDefault: vi.fn() };
      closeHandler?.(event);
      return event;
    },
  } satisfies Backend & { requestClose(): CloseRequestEvent & { preventDefault: ReturnType<typeof vi.fn> } };
  return api;
}

beforeEach(() => {
  window.localStorage.setItem("mdviewer.editorMode", "split");
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("viewer and editor behavior", () => {
  it("opens Markdown, edits it, reports dirty state and saves repeatedly with opaque capabilities", async () => {
    const api = backend();
    api.saveDocument
      .mockResolvedValueOnce({ saved: true, writeToken: "renewed-token" })
      .mockResolvedValueOnce({ saved: true, writeToken: "renewed-again-token" });
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Abrir Markdown" }));
    const editor = await screen.findByRole("textbox", { name: "Editor Markdown" });
    expect(editor).toHaveValue("# Original\n\nA [safe](https://example.com) link.");
    expect(screen.getByText("notes.md")).toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "# Changed" } });
    expect(screen.getByText("Cambios sin guardar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Guardar", exact: true }));

    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledWith("write-token", "# Changed"));
    expect(screen.getByText("Guardado")).toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "# Changed again" } });
    fireEvent.click(screen.getByRole("button", { name: "Guardar", exact: true }));
    await waitFor(() => expect(api.saveDocument).toHaveBeenLastCalledWith("renewed-token", "# Changed again"));
    expect(api.selectSaveDocument).not.toHaveBeenCalled();
  });

  it("keeps edits made while Save is pending dirty after the submitted snapshot succeeds", async () => {
    const api = backend();
    const pending = deferred<{ saved: boolean; writeToken: string }>();
    api.saveDocument.mockReturnValueOnce(pending.promise);
    render(<App backend={api} />);
    fireEvent.click(screen.getByRole("button", { name: "Abrir Markdown" }));
    const editor = await screen.findByRole("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, { target: { value: "submitted" } });
    fireEvent.click(screen.getByRole("button", { name: "Guardar", exact: true }));
    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledWith("write-token", "submitted"));
    expect(screen.getByRole("button", { name: "Guardar", exact: true })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Guardar como" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Abrir Markdown" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Convertir archivo" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Guardar como" }));
    expect(api.selectSaveDocument).not.toHaveBeenCalled();
    expect(api.saveDocument).toHaveBeenCalledTimes(1);

    fireEvent.change(editor, { target: { value: "edited while saving" } });
    await act(() => pending.resolve({ saved: true, writeToken: "renewed" }));

    expect(editor).toHaveValue("edited while saving");
    expect(screen.getByText("Cambios sin guardar")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Guardar", exact: true }));
    await waitFor(() => expect(api.saveDocument).toHaveBeenLastCalledWith("renewed", "edited while saving"));
  });

  it("keeps edits made while Save As is pending dirty and retains the selected name", async () => {
    const api = backend();
    const pending = deferred<{ saved: boolean; writeToken: string }>();
    api.saveDocument.mockReturnValueOnce(pending.promise);
    render(<App backend={api} />);
    const editor = screen.getByRole("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, { target: { value: "submitted as copy" } });
    fireEvent.click(screen.getByRole("button", { name: "Guardar como" }));
    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledWith("copy-token", "submitted as copy"));
    expect(screen.getByRole("button", { name: "Abrir Markdown" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Convertir archivo" })).toBeDisabled();

    fireEvent.change(editor, { target: { value: "edited during save as" } });
    await act(() => pending.resolve({ saved: true, writeToken: "copy-renewed" }));

    expect(screen.getByText("copy.md")).toBeInTheDocument();
    expect(editor).toHaveValue("edited during save as");
    expect(screen.getByText("Cambios sin guardar")).toBeInTheDocument();
  });

  it("keeps opened or converted document B consistent for both stale save completion orders", () => {
    const documentA: DocumentState = {
      generation: 1,
      name: "a.md",
      content: "A submitted",
      savedContent: "A",
      writeToken: "write-a",
    };
    const completions: SaveCompletion[] = [
      {
        generation: documentA.generation,
        savedContent: documentA.content,
        writeToken: "renewed-a",
      },
      {
        generation: documentA.generation,
        name: "copy-a.md",
        savedContent: documentA.content,
        writeToken: "renewed-copy-a",
      },
    ];

    for (const replacementKind of ["opened", "converted"] as const) {
      const documentB: DocumentState = {
        generation: 2,
        name: replacementKind === "opened" ? "b.md" : "converted-b.md",
        content: replacementKind === "opened" ? "B" : "# Converted B",
        savedContent: replacementKind === "opened" ? "B" : "# Converted B",
        writeToken: replacementKind === "opened" ? "write-b" : "converted-write-b",
      };

      for (const completion of completions) {
        const saveTransition = { type: "save-completed" as const, completion };
        const replacementTransition = {
          type: "replace" as const,
          replacement: documentB,
        };
        const replacementFirst = transitionDocument(
          transitionDocument(documentA, replacementTransition),
          saveTransition,
        );
        const saveFirst = transitionDocument(
          transitionDocument(documentA, saveTransition),
          replacementTransition,
        );
        expect(replacementFirst).toEqual(documentB);
        expect(saveFirst).toEqual(documentB);
        expect(replacementFirst).toMatchObject({
          name: documentB.name,
          content: documentB.content,
          savedContent: documentB.savedContent,
          writeToken: documentB.writeToken,
        });
      }
    }
  });

  it("intercepts rejected dirty close and allows the confirmed close branch", async () => {
    const api = backend();
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "Unsaved" },
    });

    await waitFor(() => expect(api.onCloseRequested).toHaveBeenCalled());
    const rejected = api.requestClose();
    expect(rejected.preventDefault).toHaveBeenCalledTimes(1);
    const confirmed = api.requestClose();
    expect(confirmed.preventDefault).not.toHaveBeenCalled();
    expect(confirm).toHaveBeenCalledTimes(2);
  });

  it("navigates find matches and selects the current occurrence in the editor", async () => {
    const api = backend();
    render(<App backend={api} />);
    const editor = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, { target: { value: "alpha beta alpha" } });

    fireEvent.keyDown(window, { key: "f", metaKey: true });
    const search = await screen.findByRole("searchbox", { name: "Buscar en el documento" });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "alpha" } });
    expect(screen.getByText("1 de 2 coincidencias")).toBeInTheDocument();
    expect(editor.selectionStart).toBe(0);
    expect(editor.selectionEnd).toBe(5);

    fireEvent.click(screen.getByRole("button", { name: "Siguiente coincidencia" }));
    expect(screen.getByText("2 de 2 coincidencias")).toBeInTheDocument();
    expect(editor.selectionStart).toBe(11);
    fireEvent.click(screen.getByRole("button", { name: "Coincidencia anterior" }));
    expect(editor.selectionStart).toBe(0);
  });

  it("keeps original UTF-16 selection offsets when case folding expands Unicode", async () => {
    const api = backend();
    render(<App backend={api} />);
    const editor = screen.getByRole<HTMLTextAreaElement>("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, { target: { value: "İstanbul İ" } });
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const search = await screen.findByRole("searchbox", { name: "Buscar en el documento" });
    fireEvent.change(search, { target: { value: "i" } });

    expect(screen.getByText("1 de 2 coincidencias")).toBeInTheDocument();
    expect(editor.selectionStart).toBe(0);
    expect(editor.selectionEnd).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "Siguiente coincidencia" }));
    expect(editor.selectionStart).toBe(9);
    expect(editor.selectionEnd).toBe(10);
  });

  it("renders a real GFM AST while disabling raw HTML and unsafe navigation", async () => {
    const api = backend();
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: {
        value: [
          "# Preview",
          "",
          "1. first",
          "2. second with **bold and *nested***",
          "",
          "- [x] done",
          "",
          "~~removed~~ and <https://example.com/auto>",
          "",
          "| A | B |",
          "|---|---|",
          "| 1 | 2 |",
          "",
          "![diagram](asset://localhost/diagram.png)",
          "",
          "`<button onclick=\"bad()\">literal</button>`",
          "",
          "```html",
          "<script>literal fenced code</script>",
          "```",
          "",
          "<a onclick=\"alert(1)\">raw</a><script>alert(1)</script>",
          "",
          "[unsafe](javascript:alert(1)) [scheme-relative](//evil.example/x) [relative](docs/readme.md) [section](#preview) [outside](https://example.com)",
        ].join("\n"),
      },
    });

    const preview = screen.getByRole("region", { name: "Vista previa" });
    expect(within(preview).getByRole("heading", { name: "Preview" })).toBeInTheDocument();
    expect(within(preview).getAllByRole("list")[0].tagName).toBe("OL");
    expect(preview.querySelector("del")).toHaveTextContent("removed");
    expect(within(preview).getByRole("checkbox", { name: "done" })).toBeChecked();
    expect(within(preview).getByRole("table")).toBeInTheDocument();
    expect(within(preview).getByRole("img", { name: "diagram" })).toHaveAttribute("src", "asset://localhost/diagram.png");
    expect(within(preview).getByText('<button onclick="bad()">literal</button>')).toBeInTheDocument();
    expect(within(preview).getByText("<script>literal fenced code</script>")).toBeInTheDocument();
    expect(preview.querySelector("script")).toBeNull();
    expect(preview.querySelector("[onclick]")).toBeNull();
    for (const label of ["unsafe", "scheme-relative", "relative"]) {
      expect(within(preview).queryByRole("link", { name: label })).toBeNull();
    }
    expect(within(preview).getByRole("link", { name: "section" })).toHaveAttribute("href", "#preview");

    const outside = within(preview).getByRole("link", { name: "outside" });
    const automatic = within(preview).getByRole("link", { name: "https://example.com/auto" });
    expect(outside).not.toHaveAttribute("href");
    expect(automatic).not.toHaveAttribute("href");
    fireEvent.click(outside);
    fireEvent.keyDown(automatic, { key: "Enter" });
    expect(confirm).toHaveBeenCalledTimes(2);
    expect(api.openExternal).toHaveBeenNthCalledWith(1, "https://example.com");
    expect(api.openExternal).toHaveBeenNthCalledWith(2, "https://example.com/auto");
  });

  it("sanitizes long Unicode titles on grapheme boundaries and preserves one Markdown suffix", () => {
    const family = "👨‍👩‍👧‍👦";
    const name = markdownName(`${"Información ".repeat(30)}${family.repeat(30)}.MD`);
    expect(name.toLocaleLowerCase().endsWith(".md")).toBe(true);
    expect(name.toLocaleLowerCase().endsWith(".md.md")).toBe(false);
    expect([...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(name)].length).toBeLessThanOrEqual(120);
    expect(name).not.toMatch(/[\uD800-\uDBFF]$/);
  });

  it("persists light, dark and system theme preference locally", () => {
    const api = backend();
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("combobox", { name: "Tema" }), { target: { value: "dark" } });
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
    expect(window.localStorage.getItem("mdviewer.theme")).toBe("dark");
  });

  it("exports standalone sanitized HTML through an opaque format-scoped capability", async () => {
    const api = backend();
    const fetchAsset = vi.fn();
    vi.stubGlobal("fetch", fetchAsset);
    api.selectOpenDocument.mockResolvedValueOnce({
      name: 'Quarterly <Report & "x">.md',
      readToken: "read-token",
      writeToken: "write-token",
    });
    render(<App backend={api} />);
    fireEvent.click(screen.getByRole("button", { name: "Abrir Markdown" }));
    const editor = await screen.findByRole("textbox", { name: "Editor Markdown" });
    fireEvent.change(editor, {
      target: {
        value: [
          "# Export <script>alert(1)</script>",
          "",
          "[safe](https://example.com/path?q=1#part)",
          "[unsafe](javascript:alert(1))",
          "![remote](https://example.com/tracker.png)",
          "",
          "<img src=x onerror=alert(1)>",
        ].join("\n"),
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));

    await waitFor(() => expect(api.selectExportDocument).toHaveBeenCalledWith('Quarterly <Report & "x">.html', "html"));
    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledTimes(1));
    const [token, html] = api.saveDocument.mock.calls[0];
    expect(token).toBe("html-token");
    expect(html).toMatch(/^<!doctype html>\n<html lang="es">\n<head>\n<meta charset="utf-8">/);
    expect(html).toContain("<title>Quarterly &lt;Report &amp; &quot;x&quot;&gt;.md</title>");
    expect(html).toContain("<style>");
    expect(html).toContain('<main class="markdown-body">');
    expect(html).toContain('href="https://example.com/path?q=1#part"');
    expect(html).not.toContain("data-export-href");
    const exportedMarkup = new DOMParser().parseFromString(html, "text/html").querySelector("main")?.outerHTML ?? "";
    expect(exportedMarkup).not.toMatch(/<script|\son[a-z]+\s*=|javascript:|data:/i);
    expect(fetchAsset).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("inlines bounded local preview images without network access", async () => {
    const api = backend();
    const fetchAsset = vi.fn().mockResolvedValue(new Response(new Uint8Array([137, 80, 78, 71]), {
      status: 200,
      headers: { "content-type": "image/png", "content-length": "4" },
    }));
    vi.stubGlobal("fetch", fetchAsset);
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "![diagram](asset://localhost/diagram.png)" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));

    await waitFor(() => expect(api.saveDocument).toHaveBeenCalledTimes(1));
    expect(fetchAsset).toHaveBeenCalledWith("asset://localhost/diagram.png");
    expect(api.saveDocument.mock.calls[0][1]).toContain('src="data:image/png;base64,iVBORw=="');
  });

  it("rejects oversized local export images before selecting an HTML destination", async () => {
    const api = backend();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(new Uint8Array([1]), {
      status: 200,
      headers: { "content-type": "image/png", "content-length": String(5 * 1024 * 1024 + 1) },
    })));
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "![huge](asset://localhost/huge.png)" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo exportar el documento como HTML.");
    expect(api.selectExportDocument).not.toHaveBeenCalled();
    expect(api.saveDocument).not.toHaveBeenCalled();
  });

  it("bounds total standalone image bytes even when one local asset is repeated", async () => {
    const api = backend();
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(new Uint8Array(1024 * 1024), {
      status: 200,
      headers: { "content-type": "image/png", "content-length": String(1024 * 1024) },
    })));
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: Array.from({ length: 21 }, () => "![repeat](asset://localhost/repeat.png)").join("\n") },
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo exportar el documento como HTML.");
    expect(api.selectExportDocument).not.toHaveBeenCalled();
    expect(api.saveDocument).not.toHaveBeenCalled();
  });

  it("keeps HTML export cancellation silent and reports transactional write failures", async () => {
    const api = backend();
    api.selectExportDocument.mockResolvedValueOnce(null).mockResolvedValueOnce({
      name: "failed.html",
      writeToken: "failed-token",
    });
    api.saveDocument.mockRejectedValueOnce({ code: "save_failed" });
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));
    await waitFor(() => expect(api.selectExportDocument).toHaveBeenCalledTimes(1));
    expect(api.saveDocument).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo exportar el documento como HTML.");
  });

  it("disables conflicting actions while HTML export is pending", async () => {
    const api = backend();
    const pending = deferred<{ name: string; writeToken: string } | null>();
    api.selectExportDocument.mockReturnValueOnce(pending.promise);
    render(<App backend={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Exportar HTML" }));
    expect(await screen.findByText("Exportando HTML…")).toBeInTheDocument();
    for (const name of ["Abrir Markdown", "Guardar como", "Convertir archivo", "Exportar HTML", "Exportar PDF"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    await act(() => pending.resolve(null));
    expect(screen.queryByText("Exportando HTML…")).not.toBeInTheDocument();
  });

  it("exports PDF through the universal native print dialog and ships a preview-only print stylesheet", () => {
    const api = backend();
    document.documentElement.dataset.theme = "dark";
    render(<App backend={api} />);
    fireEvent.change(screen.getByRole("textbox", { name: "Editor Markdown" }), {
      target: { value: "# Print\n\n```txt\ndark code\n```\n\n| A |\n|---|\n| B |" },
    });
    document.title = "MDViewer shell";
    let printSnapshot: Record<string, string> | undefined;
    const print = vi.spyOn(window, "print").mockImplementation(() => {
      const editor = document.querySelector<HTMLElement>(".editor-pane");
      const preview = document.querySelector<HTMLElement>(".preview-pane");
      const code = preview?.querySelector<HTMLElement>("code");
      const heading = preview?.querySelector<HTMLElement>("th");
      if (!editor || !preview || !code || !heading) throw new Error("print fixture unavailable");
      printSnapshot = {
        printing: document.documentElement.dataset.mdviewerPrinting ?? "",
        title: document.title,
        editorDisplay: getComputedStyle(editor).display,
        previewDisplay: getComputedStyle(preview).display,
        previewColor: getComputedStyle(preview).color,
        codeBackground: getComputedStyle(code).backgroundColor,
        headingBackground: getComputedStyle(heading).backgroundColor,
      };
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar PDF" }));

    expect(print).toHaveBeenCalledTimes(1);
    expect(printSnapshot).toEqual({
      printing: "true",
      title: "Sin título",
      editorDisplay: "none",
      previewDisplay: "block",
      previewColor: "rgb(0, 0, 0)",
      codeBackground: "rgb(243, 244, 246)",
      headingBackground: "rgb(243, 244, 246)",
    });
    expect(document.documentElement).not.toHaveAttribute("data-mdviewer-printing");
    expect(document.title).toBe("MDViewer shell");
    const stylesheet = document.querySelector("style[data-mdviewer-print]");
    expect(stylesheet).toHaveTextContent("@media print");
  });

  it("restores print state and title and surfaces an error when the native dialog throws", async () => {
    const api = backend();
    render(<App backend={api} />);
    document.title = "MDViewer shell";
    let printSnapshot: { printing: string; title: string } | undefined;
    vi.spyOn(window, "print").mockImplementation(() => {
      printSnapshot = {
        printing: document.documentElement.dataset.mdviewerPrinting ?? "",
        title: document.title,
      };
      throw new Error("native print unavailable");
    });

    fireEvent.click(screen.getByRole("button", { name: "Exportar PDF" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("No se pudo abrir el diálogo de impresión.");
    expect(printSnapshot).toEqual({ printing: "true", title: "Sin título" });
    expect(document.documentElement).not.toHaveAttribute("data-mdviewer-printing");
    expect(document.title).toBe("MDViewer shell");
  });
});
