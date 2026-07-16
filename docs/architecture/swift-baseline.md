# Swift migration baseline

The last buildable Swift 6.2 macOS application is archived by the annotated
`swift-baseline-final` tag. That tag resolves to pre-removal commit
`8b6d73427693251e5cee7e786dc500013f862815`, where `legacy/macos-swift/` still
contains the mechanically relocated Swift sources, tests, Xcode project,
packaging assets and release scripts.

## Verification

Create an isolated worktree at the archived tag, then run:

```bash
git worktree add ../mdviewer-swift-baseline swift-baseline-final
swift test --package-path ../mdviewer-swift-baseline/legacy/macos-swift
swift build --package-path ../mdviewer-swift-baseline/legacy/macos-swift
```

The frozen test baseline is 90 tests with 0 failures. It was rerun immediately
before the baseline tag and retirement commit.

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

Network-backed YouTube import is present only in the archived Swift baseline. It
is explicitly excluded from the cross-platform v1 because the agreed v1
conversion flow is local and does not require network access. The legacy
converter remains unchanged at `swift-baseline-final`; its archived presence does
not make YouTube part of the new v1 scope.
