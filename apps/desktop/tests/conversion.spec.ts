import { expect, test } from "@playwright/test";

test("converts through the typed boundary and opens the result with warnings", async ({ page }) => {
  await page.addInitScript(() => {
    window.confirm = () => true;
    window.__MDVIEWER_TEST_BACKEND__ = {
      selectOpenDocument: async () => null,
      selectSaveDocument: async () => ({ name: "converted.md", writeToken: "output" }),
      selectConversionSource: async () => ({ name: "source.pdf", readToken: "source" }),
      openDocument: async () => ({ content: "# Converted in browser" }),
      saveDocument: async () => ({ saved: true, writeToken: "renewed-write" }),
      convertDocument: async (request) => ({
        operationId: request.operationId,
        markdownToken: "markdown",
        warningCodes: ["table_degraded"],
        writeToken: "converted-write",
      }),
      cancelConversion: async () => undefined,
      claimPrintJob: async () => { throw new Error("unused"); },
      finishPrintJob: async () => undefined,
      integrationStatus: async () => ({ pendingPrintJobIds: [] }),
      activateWindow: async () => undefined,
      openExternal: async () => undefined,
      onPrintJobRequested: async () => () => undefined,
      onCloseRequested: async () => () => undefined,
    };
  });
  await page.goto("/");

  await page.getByRole("button", { name: "Convertir archivo" }).click();
  await expect(page.getByRole("textbox", { name: "Editor Markdown" })).toHaveValue("# Converted in browser");
  await expect(page.getByText("La tabla se simplificó durante la conversión.")).toBeVisible();
  await expect(page.getByText("converted.md")).toBeVisible();
});
