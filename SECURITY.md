# Security policy

## Supported versions

Security fixes are provided for the latest published MDViewer v1.2 release. The public binary
supports macOS 13 or later on Apple Silicon. Windows and Linux are compile-tested but do not yet
have published binary releases.

## Reporting a vulnerability

Please use the repository's private
[GitHub security advisory form](https://github.com/mandalorianuy/mdviewer/security/advisories/new).
Do not include private documents, credentials or exploit details in a public issue. Include the
affected commit/version, platform, reproduction steps and expected impact. Maintainers will
acknowledge a complete report as soon as practical and coordinate disclosure after a fix exists.

## Security boundaries

- Conversion is local-only. v1.2 does not fetch URLs or upload document contents. OCR uses the
  platform-native or packaged local backend and receives only the bounded image/page being
  converted: Apple Vision on macOS, Windows Media OCR on Windows and the Tesseract library API on
  Linux.
- The renderer WebView receives opaque job and write tokens, not source filesystem paths.
- PDFium is pinned by release and SHA-256, downloaded outside Git and embedded in the macOS bundle.
- The macOS PDF Service is a native Finder alias to the exact application bundle. The alias is not
  itself executable or separately signed; lifecycle checks require the resolved bundle and nested
  PDFium runtime to satisfy the release signature contract.
- Production release scripts reject dirty trees, modified lockfiles, unsigned code, non-arm64
  artifacts, absent checksum receipts and hardcoded Apple credentials.
