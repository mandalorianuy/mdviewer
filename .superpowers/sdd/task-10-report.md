# Task 10 Implementation Report

## Scope and result

Implemented Task 10 only: bounded local conversion for ZIP, EPUB, DOCX, PPTX,
XLSX, PNG, and JPEG. The local v1 registry is explicit and contains PDF,
HTML, CSV, JSON, XML, ZIP, EPUB, DOCX, PPTX, XLSX, PNG, and JPEG. No UI, CLI,
job system, release flow, OCR engine, or network client was added.

All converters return the shared `mdconvert_core::Document` model. The checked-in
goldens are produced by the shared GFM emitter; format converters do not build
Markdown strings.

## Implemented behavior

- ZIP: validates the central directory and matching local headers entirely in
  memory, sorts normalized names deterministically, converts the first local
  CSV/JSON/XML/HTML entry, warns when additional convertible entries are
  skipped, and otherwise emits a deterministic inventory.
- EPUB: requires the literal first stored entry to be the exact EPUB mimetype,
  resolves `container.xml`, OPF manifest/spine/navigation, and local assets
  inside the package, validates XHTML as strict XML, then reuses the bounded
  HTML byte converter.
- DOCX: reads relationships, metadata, styles with `basedOn` inheritance,
  numbering, headings, nested lists, paragraphs, bold/italic runs, tables,
  safe local links, and local images.
- PPTX: follows presentation relationship order, preserves slide titles/body,
  tables, local images, safe run links, notes, and page count.
- XLSX: follows workbook relationship order, supports shared and inline
  strings, percentage display styles, and emits formulas as code alongside
  cached displayed values. Formula contents are never evaluated.
- PNG/JPEG: preserves original local image assets, dimensions, and bounded
  semantic metadata without decoding pixels. Pixel-only images emit
  `OcrDeferred`; Task 10 never invokes or requires OCR.

The HTML crate gained a bounded `convert_bytes` entry point so EPUB and ZIP can
reuse its semantic conversion without writing package members to disk.

## Security properties

The archive reader performs no filesystem extraction. It rejects NUL names,
absolute/drive/UNC paths, `..`, duplicate normalized paths, symlinks and other
special entries, encryption, unsupported flags/compression, AES/ZIP64,
central/local metadata disagreement, CRC mismatch, truncation, corrupt EOCD,
deflate trailing bytes, and nested archives. Configurable non-zero limits cover
entry count, per-entry compressed/uncompressed bytes, total expanded bytes, and
expansion ratio. EPUB and OOXML relationship resolution is package-local and
cannot escape the archive root. EPUB asset limits are cumulative across spine
items. XLSX external-link parts fail closed. External package links/assets are
not fetched.

The production dependency graph passes:

```text
! cargo tree --workspace | rg -i 'reqwest|hyper|curl'
```

No advisory scanner is installed: `cargo audit --version` and
`cargo deny --version` both report `no such command`. No network installation
was attempted, so advisory-database validation is unavailable rather than
claimed as passed.

## TDD evidence

Initial RED:

```text
cargo test -p mdconvert-formats --test container_formats --test image_without_ocr
```

Failed to compile because the Task 10 converter APIs did not exist. Subsequent
focused RED cases demonstrated rejection gaps for local-header size mismatch,
EPUB cumulative assets, PPTX run links, XLSX external-link parts, EPUB first
entry ordering, DOCX inherited styles, deflate trailing bytes, PNG iTXt, and
generic ZIP selection/warnings. Each focused case failed before its production
change and passed afterward.

Final focused ZIP GREEN:

```text
cargo test -p mdconvert-formats --test container_formats \
  zip_converts_the_first_supported_entry_in_normalized_order_without_extraction -- --exact
```

Result: 1 passed, 0 failed.

## Final validation

- `cargo test -p mdconvert-formats`: 52 passed, 0 failed (20 container, 5 image,
  27 structured-format tests).
- `cargo test -p mdconvert-core --test model_contract`: 16 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- Network dependency gate above: passed.
- `PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" ./scripts/verify-workspace.sh`:
  passed, including 194 Rust tests, doc tests, TypeScript checking, 3 frontend
  tests, and the production frontend build.
- `./scripts/verify-legacy-swift.sh`: 90 executed, 0 failures.
- `git diff --check`: passed.

## Fixtures and goldens

Portable authored fixtures:

| File | SHA-256 |
| --- | --- |
| `bounded.zip` | `69cd980ef996f02fd33f59f2cb9e91ba5df72d0265076a96edfa1d795a53bcfd` |
| `semantic.docx` | `0f45feccbf38c601c691b9cbac779b8a93f425734b9eafccf13b312093d675aa` |
| `ordered.pptx` | `504277327962f2b45dc20d3b2776f120a49b50903f29115da9704cdefff0fd6f` |
| `displayed.xlsx` | `398c7261e89d86bd73d987cdfb66d2aa15dbbcd7009e64274b68d766924bf396` |
| `spine.epub` | `515b5b8c96399c79f79226b19886a6bfd6a1eb3b8bcc95bdbec94592669c3b9b` |
| `metadata.png` | `f04e818d291f20fbe6dec8ec1ad52452a7c810a9caa944d35478b57ba40bfbc9` |
| `metadata.jpg` | `82c99cea987ee571b2c0a6bd538a302dac359bf44e6eb9baa40d7748091d2547` |

Shared-emitter goldens:

| File | SHA-256 |
| --- | --- |
| `bounded.zip.md` | `238df5ea782e7bbb8dc6b7eaeebc72aca6cfa0e3f573bcda76a9c7d487a4b43b` |
| `semantic.docx.md` | `d05d0a342e9a5f61cecfae27fd3b79b6fbe91388d62cb3549804b1070c96489d` |
| `ordered.pptx.md` | `d904be79359fcb001eff7915475aa5f168da3d3d3c595d830532e5aee560f214` |
| `displayed.xlsx.md` | `f1ddd1811e01afe7739075aa5c78714a2a6eab5ad1110cbb48a3a9332b28d15d` |
| `spine.epub.md` | `c1cd011d226f70bb8b9a744a2942cdf0ee01323b1fe3875c272cae8b0a389ef3` |
| `metadata.png.md` | `65e169ea70f1c364f4e8f3aee620b98ba0d91384d537890b6f84fed583fcd281` |
| `metadata.jpg.md` | `034e39df56dae237605dc09cc387b1be4d8aedcf76108f85bab974822a17e2f6` |

## Known limitations and residual risk

- Generic ZIP v1 converts only a top-level CSV, JSON, XML, HTML, or HTM entry;
  unsupported-only archives remain inventories and nested archives fail closed.
- EPUB XHTML is intentionally stricter than permissive browser HTML because it
  is validated as XML before semantic conversion.
- OOXML support targets the Task 10 semantic subset, not full rendering parity
  with Office applications. Unsupported layout/style detail is not invented.
- XLSX cached values are trusted only as package data and formulas are not
  recalculated.
- Image v1 supports PNG/JPEG dimensions and bounded embedded metadata only;
  there is no pixel OCR or general EXIF rendering pipeline.
- The unavailable advisory scanner is the only validation gap recorded above.
