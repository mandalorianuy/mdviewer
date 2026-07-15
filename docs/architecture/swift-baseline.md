# Swift migration baseline

The buildable Swift 6.2 macOS application is frozen under
`legacy/macos-swift/` while the cross-platform replacement is developed. The
relocation is mechanical: Swift sources, tests, Xcode project, packaging assets,
and release scripts retain their pre-migration behavior.

## Verification

From the repository root, run:

```bash
./scripts/verify-legacy-swift.sh
swift build --package-path legacy/macos-swift
```

The frozen test baseline is 90 tests with 0 failures. The root-level verification
script is the deterministic gate used throughout the migration.

## Frozen feature inventory

- Open, edit, save, search, and render Markdown with GFM-compatible preview.
- Preview-only, editor-only, and split editor/preview modes.
- Font family, font size, system/light/dark appearance, and tabbed-window
  preferences.
- Markdown and convertible-file associations on macOS.
- HTML and PDF export.
- Local conversion to Markdown for CSV, JSON, XML, HTML, ZIP, PDF, images,
  EPUB, DOCX, PPTX, and XLSX, including surfaced conversion warnings and Save
  as Markdown.
- Legacy image OCR through the macOS Vision framework.
- Legacy YouTube URL and `.webloc` conversion, including transcript fallback
  behavior.
- macOS app packaging, App Store archive support, Developer ID signing,
  notarization, installation, and Gatekeeper verification scripts.

## v1 exclusion

Network-backed YouTube import is present only in this frozen Swift baseline. It
is explicitly excluded from the cross-platform v1 because the agreed v1
conversion flow is local and does not require network access. The legacy
converter remains unchanged as comparison evidence; its presence does not make
YouTube part of the new v1 scope.
