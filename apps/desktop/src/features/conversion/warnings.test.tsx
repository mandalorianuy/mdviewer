import { describe, expect, it } from "vitest";

import { warningMessage } from "./warnings";

describe("OCR warning messages", () => {
  it("keeps no-text and low-confidence outcomes distinct and reviewable", () => {
    expect(warningMessage("ocr_no_text_found")).toContain("no encontró texto");
    expect(warningMessage("ocr_low_confidence")).toContain("baja confianza");
    expect(warningMessage("ocr_deferred")).toContain("no está disponible");
  });
});
