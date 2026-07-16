# Contributing to MDViewer

MDViewer is an MIT-licensed Tauri 2 application with a portable Rust conversion core. Contributions
must keep conversion local and deterministic and must not add network-backed conversion, OCR or
machine-learning runtimes to v1.

## Development setup

Install Node.js 24, Rust 1.94 through `rustup`, and the Tauri prerequisites for your platform. Then:

```bash
npm ci
./scripts/verify-workspace.sh
```

PDF extraction tests on Apple Silicon use the pinned, checksum-verified runtime outside Git:

```bash
./scripts/fetch-pdfium.sh
PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" \
  cargo test -p mdconvert-pdf
```

Windows and Linux contributors can build the PDF crate through dynamic loading without downloading
the unpublished macOS runtime:

```bash
cargo check -p mdconvert-pdf --all-targets
cargo test -p mdconvert-pdf --test pdf_conversion
```

## Change discipline

- Add a failing test before behavior changes, then make it pass.
- Preserve the `mdviewer.convert/v1` result contract unless a new schema version is intentional.
- Do not commit `.cache/`, signing material, notarization credentials or generated release output.
- Run `./scripts/test-release-scripts.sh` when changing CI, packaging or release configuration.
- Keep platform-specific adapters behind the portable core contracts.

Open a pull request against `main` with the relevant test output and known platform limitations.
Security issues must follow [SECURITY.md](SECURITY.md), not a public issue.
