import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";

afterEach(cleanup);

describe("desktop shell", () => {
  it("renders the document workspace and conversion entry point", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "MDViewer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Abrir Markdown" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Convertir archivo" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Editor Markdown" })).toBeInTheDocument();
  });
});
