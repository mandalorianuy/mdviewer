import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import App from "./App";

afterEach(cleanup);

describe("desktop shell", () => {
  it("renders the product title", () => {
    render(<App />);

    expect(screen.getByRole("heading", { name: "MDViewer" })).toBeInTheDocument();
  });

  it("renders the open action", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: "Abrir" })).toBeInTheDocument();
  });

  it("renders an empty editor region", () => {
    render(<App />);

    expect(screen.getByRole("region", { name: "Editor" })).toBeEmptyDOMElement();
  });
});
