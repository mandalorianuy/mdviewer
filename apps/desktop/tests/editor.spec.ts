import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.confirm = () => true;
    window.__MDVIEWER_TEST_BACKEND__ = {
      selectOpenDocument: async () => ({ name: "e2e.md", readToken: "read", writeToken: "write" }),
      selectSaveDocument: async (name: string) => ({ name, writeToken: "save" }),
      selectExportDocument: async (name: string) => ({ name, writeToken: "export" }),
      selectConversionSource: async () => null,
      openDocument: async () => ({ content: "# Browser E2E\n\n[unsafe](javascript:alert(1))" }),
      saveDocument: async () => ({ saved: true, writeToken: "renewed-write" }),
      convertDocument: async () => { throw new Error("unused"); },
      cancelConversion: async () => undefined,
      claimPrintJob: async () => { throw new Error("unused"); },
      finishPrintJob: async () => undefined,
      integrationStatus: async () => ({ pendingPrintJobIds: [] }),
      macosWorkflowStatus: async () => "not_installed" as const,
      installMacosWorkflow: async () => "installed" as const,
      repairMacosWorkflow: async () => "installed" as const,
      uninstallMacosWorkflow: async () => "not_installed" as const,
      macosVirtualPrinterStatus: async () => "not_installed" as const,
      installMacosVirtualPrinter: async () => "installed" as const,
      repairMacosVirtualPrinter: async () => "installed" as const,
      uninstallMacosVirtualPrinter: async () => "not_installed" as const,
      activateWindow: async () => undefined,
      openExternal: async () => undefined,
      onPrintJobRequested: async () => () => undefined,
      onCloseRequested: async () => () => undefined,
    };
  });
  await page.goto("/");
});

test("opens, edits, saves and sanitizes preview in the running app", async ({ page }) => {
  const workspace = await page.locator(".workspace").boundingBox();
  expect(workspace?.height).toBeGreaterThan(400);
  await page.getByRole("button", { name: "Abrir Markdown" }).click();
  const editor = page.getByRole("textbox", { name: "Editor Markdown" });
  await expect(editor).toHaveValue(/Browser E2E/);
  await expect(page.getByRole("region", { name: "Vista previa" }).getByRole("heading", { name: "Browser E2E" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Vista previa" }).getByRole("link", { name: "unsafe" })).toHaveCount(0);

  await editor.fill("# Changed in Chromium");
  await expect(page.getByText("Cambios sin guardar")).toBeVisible();
  await page.getByRole("button", { name: "Guardar", exact: true }).click();
  await expect(page.getByText("Guardado")).toBeVisible();
});

test("opens find from the platform keyboard shortcut", async ({ page }) => {
  const editor = page.getByRole("textbox", { name: "Editor Markdown" });
  await editor.fill("alpha beta alpha");
  const modifier = await page.evaluate(() => navigator.platform.startsWith("Mac") ? "Meta" : "Control");
  await page.keyboard.press(`${modifier}+f`);
  const search = page.getByRole("searchbox", { name: "Buscar en el documento" });
  await expect(search).toBeFocused();
  await search.fill("alpha");
  await expect(page.getByText("2 coincidencias")).toBeVisible();
});
