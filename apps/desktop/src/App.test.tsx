import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("desktop shell", () => {
  it("opens in the preview-first workspace used by the native baseline", () => {
    render(<App />);

    expect(screen.getByRole("toolbar", { name: "Controles del documento" })).toBeInTheDocument();
    expect(screen.getByText("Abrir archivo")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Convertir archivo" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Vista previa" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Editor Markdown" })).not.toBeInTheDocument();
  });

  it("switches between preview, editor and split modes without losing either surface", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Editor" }));
    expect(screen.getByRole("textbox", { name: "Editor Markdown" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Vista previa" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Dividido" }));
    expect(screen.getByRole("textbox", { name: "Editor Markdown" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Vista previa" })).toBeInTheDocument();
  });

  it("restores reader typography controls from the native baseline", () => {
    render(<App />);

    const family = screen.getByRole("combobox", { name: "Fuente de lectura" });
    const size = screen.getByRole("slider", { name: "Tamaño de lectura" });
    fireEvent.change(family, { target: { value: "serif" } });
    fireEvent.change(size, { target: { value: "19" } });

    const shell = screen.getByTestId("app-shell");
    expect(shell).toHaveStyle({ "--reader-font-size": "19px" });
    expect(shell.getAttribute("style")).toContain("--reader-font-family");
  });
});
