import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

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

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
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
});
