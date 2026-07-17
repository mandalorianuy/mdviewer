# MDViewer

MDViewer converts local documents to GitHub-Flavored Markdown and opens the result in an editor and
preview. The desktop product uses Tauri 2 with a portable Rust conversion core and CLI.

The current v1.2.1 source defines production desktop packages for **macOS 13+ Apple Silicon**,
**Windows x64**, and **Linux x64**. A platform artifact is published only after its receipt passes
the native signing or runtime verification gates.

## What v1.2.1 includes

- “Guardar como Markdown con MDViewer” in the macOS print PDF menu.
- Local PDF conversion through pinned PDFium, plus HTML, text, CSV, JSON, XML, ZIP, EPUB and OOXML.
- Transactional Markdown/assets output, warnings and cancellation cleanup.
- Viewer, editor, preview, preferences and CLI using the same conversion contracts.
- On-device OCR for PNG/JPEG images, scanned PDFs and textual images embedded in digital PDFs.
- Apple Vision on macOS, Windows Media OCR on Windows, and Tesseract 5 (`eng+spa`) on Linux.
- No uploads, network conversion or YAML frontmatter by default.

OCR runs only when an image lacks semantic text, a PDF page has no extractable glyphs, or an
eligible textual image is embedded in a digital PDF page. Icons and small decorations are skipped,
spatially equivalent digital text is not duplicated, and the original image asset is retained. It
uses no PyTorch, Docling or remote service. Low-confidence text is preserved with an explicit
warning; a page where OCR finds no text is also reported. Printing through PDF can still lose semantic
structure and reading-order information, so fidelity depends on what the source application
preserves.

The last buildable Swift application is archived by the annotated `swift-baseline-final` tag. It was
removed from the active tree only after the strict [v1 parity report](docs/release/v1-parity-report.md)
had no required or manual row pending. YouTube import remains an explicit post-v1 exclusion rather
than evidence of local-conversion parity.

## Develop

Requirements: Node.js 24, Rust 1.94 and the Tauri prerequisites for your platform.

```bash
npm ci
./scripts/verify-workspace.sh
```

Apple Silicon PDF extraction tests use a verified cache outside Git:

```bash
./scripts/fetch-pdfium.sh
PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" \
  cargo test -p mdconvert-pdf
```

Build a local macOS artifact without making signing or notarization claims:

```bash
./scripts/package-macos-arm64.sh --unsigned-smoke
./scripts/verify-release.sh --unsigned-smoke
```

## Documentation

- [macOS print workflow](docs/user-guide/macos-print-workflow.md)
- [macOS release and unsigned smoke](docs/release/macos.md)
- [v1 parity and Swift retirement gate](docs/release/v1-parity-report.md)
- [v1.1 OCR release notes](docs/release/v1.1.md)
- [v1.2 portable OCR release notes](docs/release/v1.2.md)
- [v1.2.1 multiplatform distribution notes](docs/release/v1.2.1.md)
- [Windows and Linux packaging](docs/release/windows-linux.md)
- [CLI contract](docs/reference/cli.md)
- [cross-platform architecture](docs/superpowers/specs/2026-07-15-cross-platform-save-as-markdown-design.md)
- [archived Swift baseline](docs/architecture/swift-baseline.md)
- [contributing](CONTRIBUTING.md)
- [security](SECURITY.md)

MDViewer is licensed under the [MIT License](LICENSE).
