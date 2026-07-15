# MDViewer cross-platform Save as Markdown Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar MDViewer como aplicación Tauri 2 portable, con núcleo Rust local y determinista, y publicar primero un DMG firmado y notarizado para macOS 13+ Apple Silicon que agregue “Guardar como Markdown con MDViewer…” al menú PDF de impresión.

**Architecture:** Un monorepo conserva la aplicación Swift buildable como baseline hasta alcanzar paridad. Todos los extractores producen un modelo intermedio común; un único emisor crea GFM y un escritor transaccional publica el `.md` y sus assets. La aplicación Tauri usa ese mismo núcleo que la CLI. El PDF Workflow de macOS sólo persiste un job local y abre un deep link con UUID; no contiene lógica de conversión.

**Tech Stack:** Rust 1.94, Cargo workspace, Tauri 2.11.x, React 19.2, TypeScript 5.9, Vite 7.3, PDFium `chromium/7947` vía `pdfium-render` 0.9.3, `html5ever` 0.39, Vitest 4, Playwright 1.61, Swift 6.2 únicamente para el baseline y el adaptador nativo cuando sea necesario.

## Global Constraints

- Mantener `main` buildable al finalizar cada task.
- No retirar `legacy/macos-swift` antes de completar el gate de paridad y crear el tag de respaldo.
- No incorporar OCR, PyTorch, Docling, YouTube ni ninguna conversión con red en v1.
- No emitir YAML frontmatter por defecto.
- No agregar binarios PDFium al historial Git; descargarlos por script con versión y SHA-256 fijados.
- No publicar binarios Windows o Linux en v1, aunque core, CLI y desktop deben compilar allí en CI.
- No tocar ni agregar al índice el archivo preexistente no versionado `docs/superpowers/plans/2026-06-17-fase2-fase3-menu-convertir-markdown-plan.md`.
- Desarrollar cada comportamiento con test rojo, implementación mínima, test verde y commit dedicado.
- Ejecutar `git status --short` antes de cada commit y agregar sólo las rutas de la task activa.
- No codificar IDs, issuers, rutas de claves ni credenciales de firma; toda publicación los recibe por variables de entorno o GitHub Secrets.

---

## Target Repository Map

```text
/
├── Cargo.toml                         Cargo workspace y dependencias comunes
├── Cargo.lock
├── package.json                       Workspace npm y comandos raíz
├── package-lock.json
├── rust-toolchain.toml                Rust 1.94 con rustfmt y clippy
├── apps/desktop/                      Tauri 2 + React/TypeScript
├── crates/mdconvert-core/             Modelo, GFM, assets y escritura atómica
├── crates/mdconvert-html/             HTML5 DOM a Document
├── crates/mdconvert-pdf/              PDFium, layout y heurísticas
├── crates/mdconvert-formats/          CSV, JSON, XML, ZIP, EPUB y OOXML
├── crates/mdconvert-cli/              CLI y JSON de resultados v1
├── platform/macos/pdf-workflow/       Executable PDF Workflow e instalador
├── legacy/macos-swift/                Aplicación y tests Swift congelados
├── scripts/                           Gates, descarga PDFium y release
├── tests/fixtures/                    Corpus portable versionado
├── tests/golden/                      GFM, assets y warnings esperados
└── docs/                              Arquitectura, usuario y release
```

## Public Contracts to Keep Stable

```rust
pub struct Document {
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
    pub assets: Vec<Asset>,
    pub warnings: Vec<ConversionWarning>,
}

pub enum Block {
    Heading { level: u8, content: Vec<Inline> },
    Paragraph { content: Vec<Inline> },
    List { ordered: bool, start: Option<u64>, items: Vec<ListItem> },
    Table { alignments: Vec<Alignment>, rows: Vec<Vec<Vec<Inline>>> },
    Code { language: Option<String>, text: String },
    Quote { blocks: Vec<Block> },
    Image { asset_id: AssetId, alt: String },
    ThematicBreak,
}

pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    Link { url: String, title: Option<String>, content: Vec<Inline> },
    LineBreak,
}

pub trait Converter: Send + Sync {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError>;
}
```

La salida programática queda versionada como `mdviewer.convert/v1`; un cambio incompatible exige otro valor de `schema_version`.

---

## Task 1: Freeze and Relocate the Swift Baseline

**Files:**

- Move: `Package.swift`, `Package.resolved`, `project.yml`, `MDViewer.xcodeproj/`, `Sources/`, `Tests/`, `macos/`, current `scripts/` → `legacy/macos-swift/`
- Create: `scripts/verify-legacy-swift.sh`
- Create: `docs/architecture/swift-baseline.md`
- Modify: `.gitignore`, `README.md`

- [ ] **Step 1: Record the baseline before moving files**

Run `swift test` and `git status --short`.

Expected: 90 tests execute with 0 failures; only the approved spec correction and the preexisting untracked June plan appear.

- [ ] **Step 2: Move the Swift product mechanically**

```bash
mkdir -p legacy/macos-swift
git mv Package.swift Package.resolved project.yml MDViewer.xcodeproj Sources Tests macos scripts legacy/macos-swift/
mkdir scripts
```

Do not change Swift source semantics in this task.

- [ ] **Step 3: Add the deterministic baseline gate**

Create `scripts/verify-legacy-swift.sh` with:

```bash
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
swift test --package-path "$ROOT/legacy/macos-swift"
```

Make it executable and document the frozen feature inventory and the known exclusion of network-backed YouTube from v1.

- [ ] **Step 4: Verify and commit**

```bash
./scripts/verify-legacy-swift.sh
swift build --package-path legacy/macos-swift
git add .gitignore README.md docs/architecture/swift-baseline.md legacy/macos-swift scripts/verify-legacy-swift.sh
git commit -m "refactor: preserve Swift app as migration baseline"
```

Expected: both commands exit 0; the same 90 tests remain green.

---

## Task 2: Scaffold the Portable Workspace and Empty Desktop Shell

**Files:**

- Create: `Cargo.toml`, `rust-toolchain.toml`, `package.json`
- Create: `apps/desktop/package.json`, `apps/desktop/vite.config.ts`, `apps/desktop/tsconfig.json`
- Create: `apps/desktop/index.html`, `apps/desktop/src/main.tsx`, `apps/desktop/src/App.tsx`, `apps/desktop/src/App.test.tsx`
- Create: `apps/desktop/src-tauri/Cargo.toml`, `build.rs`, `src/lib.rs`, `src/main.rs`, `tauri.conf.json`, `capabilities/default.json`
- Create: `scripts/verify-workspace.sh`

- [ ] **Step 1: Write a failing desktop smoke test**

Assert that the shell renders `MDViewer`, an `Abrir` action and an empty editor region. Run:

```bash
npm test --workspace @mdviewer/desktop -- --run
```

Expected: failure because the workspace and component do not exist yet.

- [ ] **Step 2: Create pinned workspace manifests**

Use Cargo resolver 2 with members under `crates/*`, `apps/desktop/src-tauri` and `platform/macos/pdf-workflow`. Pin Rust `1.94.0`; React `19.2.7`; TypeScript `5.9.3`; Vite `7.3.6`; Vitest `4.1.10`; Tauri API `2.11.1`; Tauri CLI `2.11.4`.

- [ ] **Step 3: Implement minimal shells**

`apps/desktop/src-tauri/src/lib.rs` exposes:

```rust
pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
}

pub fn run() {
    builder()
        .run(tauri::generate_context!())
        .expect("MDViewer failed to start");
}
```

Keep the capability file at minimum privilege.

- [ ] **Step 4: Add and run the combined gate**

`scripts/verify-workspace.sh` runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run check
npm test -- --run
npm run build
```

Run both verification scripts. Expected: both exit 0 and the smoke test passes.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml package.json package-lock.json apps scripts/verify-workspace.sh
git commit -m "build: scaffold Tauri workspace and desktop shell"
```

---

## Task 3: Define the Intermediate Document Model

**Files:**

- Create: `crates/mdconvert-core/Cargo.toml`
- Create: `crates/mdconvert-core/src/{lib,model,converter,error}.rs`
- Create: `crates/mdconvert-core/tests/model_contract.rs`

- [ ] **Step 1: Write failing contract tests**

Test serialization round trips, heading levels restricted to 1 through 6, nonempty asset IDs, stable warning codes and rejection of an empty source path.

```rust
#[test]
fn rejects_invalid_heading_level() {
    let result = Block::heading(7, vec![Inline::Text("Título".into())]);
    assert!(matches!(result, Err(ModelError::InvalidHeadingLevel(7))));
}
```

Run `cargo test -p mdconvert-core --test model_contract`. Expected: compile failure because the crate does not exist.

- [ ] **Step 2: Implement the public model and errors**

Add the shared contracts plus:

```rust
pub struct ConversionRequest {
    pub source: PathBuf,
    pub source_url: Option<url::Url>,
    pub limits: ConversionLimits,
}

pub struct ConversionLimits {
    pub max_input_bytes: u64,
    pub max_pages: u32,
    pub max_assets: u32,
}

pub struct ConversionWarning {
    pub code: WarningCode,
    pub message: String,
    pub page: Option<u32>,
}
```

Defaults: 500 MiB input, 2,000 pages and 10,000 assets. Derive serialization, equality and debugging where applicable.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p mdconvert-core
cargo clippy -p mdconvert-core --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/mdconvert-core
git commit -m "feat(core): define portable document model"
```

---

## Task 4: Build the Single GFM Emitter

**Files:**

- Create: `crates/mdconvert-core/src/gfm.rs`
- Create: `crates/mdconvert-core/tests/gfm_emitter.rs`
- Create: `tests/golden/core/all-blocks.md`

- [ ] **Step 1: Write golden tests first**

Cover every block and inline variant, nested lists, ordered list starts, table pipes and line breaks, backticks inside code spans, link parentheses, UTF-8 and LF-only output. Assert no YAML delimiter at byte zero.

```rust
#[test]
fn escapes_table_cells_without_breaking_gfm() {
    let markdown = emit(&document_with_table_cell("a|b\nc"));
    assert!(markdown.contains("a\\|b<br>c"));
}
```

- [ ] **Step 2: Implement deterministic emission**

Expose only:

```rust
pub struct GfmOptions {
    pub final_newline: bool,
}

pub fn emit_gfm(document: &Document, options: &GfmOptions) -> Result<String, EmitError>;
```

Centralize escaping by context. Normalize line endings to LF. Render missing table cells as empty and reject irreconcilable alignment widths.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p mdconvert-core --test gfm_emitter
git diff --exit-code -- tests/golden/core/all-blocks.md
git add crates/mdconvert-core tests/golden/core
git commit -m "feat(core): emit deterministic GitHub Flavored Markdown"
```

---

## Task 5: Publish Markdown and Assets Atomically

**Files:**

- Create: `crates/mdconvert-core/src/output.rs`, `manifest.rs`
- Create: `crates/mdconvert-core/tests/output_transaction.rs`

- [ ] **Step 1: Write failure-path tests**

Cover successful writes, no-assets output, cancellation, unwritable destination, existing unknown assets directory, valid owned-assets replacement and cleanup after simulated rename failure.

```rust
#[test]
fn refuses_to_replace_an_unowned_assets_directory() {
    let error = writer.publish(&document_with_asset(), &target()).unwrap_err();
    assert!(matches!(error, OutputError::UnownedAssetsDirectory(_)));
}
```

- [ ] **Step 2: Implement manifest and transaction**

```rust
pub struct OutputTarget {
    pub markdown_path: PathBuf,
    pub overwrite: OverwritePolicy,
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

pub fn publish(
    document: &Document,
    target: &OutputTarget,
    cancellation: &dyn Cancellation,
) -> Result<WriteResult, OutputError>;
```

Write `.mdviewer-assets.json` with schema `mdviewer.assets/v1`, document filename and SHA-256 per asset. Stage on destination volume, fsync, rename assets before Markdown and restore a previous owned output if final rename fails.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p mdconvert-core --test output_transaction
git add Cargo.lock crates/mdconvert-core
git commit -m "feat(core): write markdown outputs atomically"
```

---

## Task 6: Convert HTML Through an HTML5 DOM

**Files:**

- Create: `crates/mdconvert-html/Cargo.toml`
- Create: `crates/mdconvert-html/src/{lib,dom,convert}.rs`
- Create: `crates/mdconvert-html/tests/html_conversion.rs`
- Create: `tests/fixtures/html/{semantic,malformed}.html`
- Create: `tests/golden/html/{semantic,malformed}.md`

- [ ] **Step 1: Port and expand HTML fixtures**

Copy the legacy local fixture; add nested lists, tables, blockquote, fenced code, image alt text, relative URLs, malformed nesting, `script`, `style`, hidden content and event handlers.

- [ ] **Step 2: Write failing DOM conversion tests**

Prove that semantic nodes become model nodes, scripts and invisible content disappear, and a base URL resolves relative links. Run `cargo test -p mdconvert-html`; expect failure because the crate is absent.

- [ ] **Step 3: Implement `HtmlConverter`**

Parse with `html5ever` 0.39 and an owned DOM:

```rust
pub struct HtmlConverter;

impl Converter for HtmlConverter {
    fn convert(&self, request: &ConversionRequest) -> Result<Document, ConversionError> {
        let dom = dom::parse_file(&request.source, request.limits.max_input_bytes)?;
        convert::document_from_dom(dom, request.source_url.as_ref())
    }
}
```

Fetch no resources. Decode bounded `data:` images. Permit local `file:` assets only inside the input directory; otherwise preserve alt text and emit `ExternalAssetSkipped`.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p mdconvert-html
cargo clippy -p mdconvert-html --all-targets -- -D warnings
git add Cargo.toml Cargo.lock crates/mdconvert-html tests/fixtures/html tests/golden/html
git commit -m "feat(html): convert HTML5 DOM into portable documents"
```

---

## Task 7: Pin PDFium and Extract Raw Page Geometry

**Files:**

- Create: `crates/mdconvert-pdf/Cargo.toml`
- Create: `crates/mdconvert-pdf/src/{lib,bindings,extract,raw}.rs`
- Create: `crates/mdconvert-pdf/tests/raw_extraction.rs`
- Create: `scripts/fetch-pdfium.sh`
- Create: `tests/fixtures/pdf/digital-basic.pdf`
- Modify: `.gitignore`

- [ ] **Step 1: Add a failing extraction contract**

Assert exact page count, dimensions, words, character boxes, font sizes and weights, image bounds and link targets for `digital-basic.pdf`.

- [ ] **Step 2: Add the verified PDFium fetcher**

Pin:

```text
Release: chromium/7947
Asset: pdfium-mac-arm64.tgz
SHA-256: aa9739354fc7bc8f200f3f3c9532bd5233298203051e094820272ccd9c997a77
URL: https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7947/pdfium-mac-arm64.tgz
```

Download into `.cache/pdfium/chromium-7947/`, verify SHA-256 before extraction and refuse an unexpected archive. Ignore `.cache/` in Git.

- [ ] **Step 3: Implement raw extraction with `pdfium-render` 0.9.3**

Keep PDFium types private. Normalize coordinates in portable raw structs:

```rust
pub struct RawPage {
    pub number: u32,
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<RawGlyph>,
    pub images: Vec<RawImage>,
    pub links: Vec<RawLink>,
    pub rules: Vec<RawRule>,
}
```

Reject encrypted PDFs without credentials, excess pages and documents with no extractable text using typed errors.

- [ ] **Step 4: Verify and commit without the library**

```bash
./scripts/fetch-pdfium.sh
PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" cargo test -p mdconvert-pdf --test raw_extraction
git add .gitignore Cargo.toml Cargo.lock crates/mdconvert-pdf scripts/fetch-pdfium.sh tests/fixtures/pdf/digital-basic.pdf
git commit -m "feat(pdf): extract PDF geometry through pinned PDFium"
```

Expected: checksum verification reports OK and all extraction assertions pass.

---

## Task 8: Reconstruct PDF Reading Order and Structure

**Files:**

- Create: `crates/mdconvert-pdf/src/{layout,heuristics,convert}.rs`
- Create: `crates/mdconvert-pdf/tests/pdf_conversion.rs`
- Create: `tests/fixtures/pdf/{two-columns,headings-lists,table-bordered,table-aligned,repeated-chrome,scanned}.pdf`
- Create: `tests/golden/pdf/`

- [ ] **Step 1: Add one red test per inference rule**

Cover line grouping, paragraph joining, dehyphenation, two-column order, heading levels, lists, bordered and borderless tables, repeated page chrome, image placement, links, ambiguous columns and scanned rejection.

- [ ] **Step 2: Implement deterministic layout passes**

```rust
pub fn reconstruct(raw: RawDocument) -> Result<Document, ConversionError> {
    let lines = group_glyphs_into_lines(raw.pages)?;
    let regions = detect_reading_regions(lines);
    let without_chrome = remove_repeated_page_chrome(regions);
    let blocks = infer_blocks(without_chrome);
    attach_assets_links_and_warnings(blocks, raw.metadata)
}
```

Put every numeric threshold in a named `HeuristicConfig` with units and test defaults. Low-confidence inference preserves text and adds a warning; it never silently discards content.

- [ ] **Step 3: Match all goldens and commit**

```bash
PDFIUM_DYNAMIC_LIB_PATH="$PWD/.cache/pdfium/chromium-7947/lib/libpdfium.dylib" cargo test -p mdconvert-pdf
git add crates/mdconvert-pdf tests/fixtures/pdf tests/golden/pdf
git commit -m "feat(pdf): reconstruct structured documents from PDF layout"
```

Expected: digital fixtures match GFM, assets and warnings; `scanned.pdf` returns `OcrRequired`; no output has artificial `## Página` headings.

---

## Task 9: Port CSV, JSON and XML Converters

**Files:**

- Create: `crates/mdconvert-formats/Cargo.toml`
- Create: `crates/mdconvert-formats/src/{lib,detect,csv,json,xml}.rs`
- Create: `crates/mdconvert-formats/tests/structured_formats.rs`
- Copy: relevant legacy fixtures → `tests/fixtures/formats/`
- Create: `tests/golden/formats/`

- [ ] **Step 1: Freeze behavior as model-level tests**

Port fixtures and assert delimiter detection, scalar types, arrays, nested objects, XML attributes, repeated elements, invalid UTF-8 and corrupt input.

- [ ] **Step 2: Implement converters without Markdown strings**

Each converter implements `Converter` and produces `Document` nodes. Detection inspects extension plus content signature and reports ambiguity instead of guessing between incompatible formats.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p mdconvert-formats --test structured_formats
git add Cargo.toml Cargo.lock crates/mdconvert-formats tests/fixtures/formats tests/golden/formats
git commit -m "feat(formats): port structured local converters"
```

Expected: all three formats match GFM goldens through the shared emitter.

---

## Task 10: Port ZIP, EPUB, OOXML and Local Image Metadata

**Files:**

- Create: `crates/mdconvert-formats/src/{archive,epub,docx,pptx,xlsx,image}.rs`
- Create: `crates/mdconvert-formats/tests/container_formats.rs`
- Create: `crates/mdconvert-formats/tests/image_without_ocr.rs`
- Copy: relevant legacy fixtures → `tests/fixtures/formats/`
- Create: corresponding `tests/golden/formats/` outputs

- [ ] **Step 1: Add security and fidelity tests**

Cover ZIP traversal and expansion limits; DOCX headings, lists, tables, images and links; PPTX slide order and notes; XLSX sheets, formulas and displayed values; EPUB spine, navigation and images; PNG/JPEG dimensions and metadata.

Prove that an image containing text does not invoke OCR and yields `OcrDeferred` when it has no semantic text.

- [ ] **Step 2: Implement bounded readers**

Reject absolute archive paths, `..`, symlinks and expanded totals over request limits. Reuse the HTML converter for EPUB XHTML and XML helpers for OOXML.

- [ ] **Step 3: Register only local v1 formats**

The registry contains PDF, HTML, CSV, JSON, XML, ZIP, EPUB, DOCX, PPTX, XLSX and local images. It contains no YouTube URL or network client dependency.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p mdconvert-formats
! cargo tree --workspace | rg -i 'reqwest|hyper|curl'
git add Cargo.lock crates/mdconvert-formats tests/fixtures/formats tests/golden/formats
git commit -m "feat(formats): port bounded document and image converters"
```

---

## Task 11: Expose the Versioned Headless CLI

**Files:**

- Create: `crates/mdconvert-cli/Cargo.toml`
- Create: `crates/mdconvert-cli/src/{lib,main,result}.rs`
- Create: `crates/mdconvert-cli/tests/cli.rs`
- Create: `docs/reference/cli.md`

- [ ] **Step 1: Write black-box CLI tests**

Test success, stdout JSON, warnings, unknown format, missing input, overwrite refusal, `--cancel-file`, no-assets output and `OcrRequired` using temporary directories.

- [ ] **Step 2: Implement the exact command**

```text
mdconvert convert <INPUT> --output <FILE.md> [--assets <DIR>] [--json] [--cancel-file <PATH>]
```

Successful JSON follows:

```json
{
  "schema_version": "mdviewer.convert/v1",
  "status": "succeeded",
  "markdown_path": "/absolute/document.md",
  "assets_path": null,
  "metadata": {},
  "warnings": []
}
```

Failures use the same envelope on stderr with `status: "failed"` and a stable error code. Never log document contents.

- [ ] **Step 3: Verify and commit**

```bash
cargo test -p mdconvert-cli
cargo run -p mdconvert-cli -- convert tests/fixtures/html/semantic.html --output /tmp/mdviewer-plan-smoke.md --json
test -f /tmp/mdviewer-plan-smoke.md
git add Cargo.lock crates/mdconvert-cli docs/reference/cli.md
git commit -m "feat(cli): expose versioned local conversion interface"
```

---

## Task 12: Implement Secure Jobs, Deep Links and Tauri Commands

**Files:**

- Create: `apps/desktop/src-tauri/src/{commands,jobs,deep_link,state}.rs`
- Create: `apps/desktop/src-tauri/tests/{jobs,commands}.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`, `capabilities/default.json`

- [ ] **Step 1: Write hostile-input tests**

Test invalid UUIDs, encoded slashes, symlink escapes, job directories outside the root, missing `input.pdf`, double claims, jobs older than 24 hours and sources outside user-selected scopes.

- [ ] **Step 2: Implement the job store**

```rust
pub struct PrintJobId(uuid::Uuid);

pub struct PrintJobStore {
    root: PathBuf,
}

impl PrintJobStore {
    pub fn stage_pdf(&self, source: &Path, title: Option<&str>) -> Result<PrintJob, JobError>;
    pub fn claim(&self, id: PrintJobId) -> Result<PrintJob, JobError>;
    pub fn finish(&self, id: PrintJobId) -> Result<(), JobError>;
    pub fn cleanup_older_than(&self, age: Duration) -> Result<CleanupReport, JobError>;
}
```

Create user-only directories and files, an atomic `mdviewer.print-job/v1` metadata file, canonicalize every access and lock claims by atomic rename.

- [ ] **Step 3: Add minimal typed Tauri commands**

Expose open, save, convert, cancel, warnings, claim print job and integration status. Return serializable `CommandError`, not raw filesystem errors. Register only `mdviewer://print/<uuid>`; reject queries, fragments and every other path.

- [ ] **Step 4: Verify and commit**

```bash
cargo test -p mdviewer-desktop
cargo clippy -p mdviewer-desktop --all-targets -- -D warnings
git add Cargo.lock apps/desktop/src-tauri
git commit -m "feat(desktop): add secure conversion jobs and deep links"
```

Expected: hostile-input tests pass and capabilities grant no shell execution or unrestricted filesystem access.

---

## Task 13: Reach Viewer, Editor and Conversion UI Parity

**Files:**

- Create: `apps/desktop/src/features/{documents,editor,preview,conversion,settings}/`
- Create: `apps/desktop/src/lib/tauri.ts`, `apps/desktop/src/styles/`
- Create: `apps/desktop/tests/{editor,conversion}.spec.ts`
- Modify: `apps/desktop/src/App.tsx`

- [ ] **Step 1: Write frontend behavior tests**

Cover open Markdown, edit, dirty state, save, Save As, close confirmation, find, current window behavior, GFM preview, theme preference, direct conversion, progress, cancellation, warnings and opening the result.

- [ ] **Step 2: Build a typed backend boundary**

All calls go through `apps/desktop/src/lib/tauri.ts` with explicit types. Components may not import `@tauri-apps/api/core` directly.

- [ ] **Step 3: Implement sanitized GFM preview**

Render GFM locally. Strip active HTML, scripts, event handlers and unsafe URLs. Keep local anchors internal; require confirmation before opening external links in the operating system.

- [ ] **Step 4: Implement conversion UX**

For a claimed print job, activate MDViewer, present native Save As with a sanitized title, then show progress and cancel. On success show warnings, open the Markdown and finish the job. On cancel or failure clean staging without partial output.

- [ ] **Step 5: Verify and commit**

```bash
npm test --workspace @mdviewer/desktop -- --run
npm run build --workspace @mdviewer/desktop
npm run test:e2e --workspace @mdviewer/desktop
./scripts/verify-legacy-swift.sh
git add apps/desktop
git commit -m "feat(desktop): reach viewer editor and conversion parity"
```

Expected: unit, build and E2E tests pass; Swift baseline remains green.

---

## Task 14: Add the macOS PDF Workflow and Integration Controls

**Files:**

- Create: `platform/macos/pdf-workflow/Cargo.toml`
- Create: `platform/macos/pdf-workflow/src/{lib,main}.rs`
- Create: `platform/macos/pdf-workflow/tests/workflow.rs`
- Create: `apps/desktop/src-tauri/src/macos_integration.rs`
- Create: `apps/desktop/src-tauri/tests/macos_integration.rs`
- Create: `apps/desktop/src/features/settings/IntegrationsPanel.tsx`
- Create: `docs/user-guide/macos-print-workflow.md`

- [ ] **Step 1: Write lifecycle and invocation tests**

Test install, status, repair, version mismatch, checksum mismatch, uninstall, invocation with CUPS options and PDF path, MDViewer closed/open and failure to persist a job.

- [ ] **Step 2: Implement the workflow executable**

Name it exactly `Guardar como Markdown con MDViewer`. Treat the final process argument as PDF path and preceding arguments as opaque CUPS options. Validate a regular readable PDF, stage through `PrintJobStore`, fsync, then open only `mdviewer://print/<uuid>` with Launch Services.

Exit 0 only after persistence and dispatch succeed. Never choose a destination or convert in this tool.

- [ ] **Step 3: Implement install, repair and uninstall**

Install per-user at:

```text
~/Library/PDF Services/Guardar como Markdown con MDViewer
```

Compare embedded version, SHA-256 and code signature before reporting `installed`. Replace atomically. Uninstall only the exact regular file whose marker and signature identify this build; never recursively remove `~/Library/PDF Services`.

- [ ] **Step 4: Connect Preferences → Integrations**

Display `not installed`, `installed`, `outdated` or `invalid`; expose Install, Repair or Uninstall with accessible confirmation and result text.

- [ ] **Step 5: Verify automated and real-system behavior**

```bash
cargo test -p mdviewer-pdf-workflow
cargo test -p mdviewer-desktop --test macos_integration
npm test --workspace @mdviewer/desktop -- --run
```

Install a development-signed build and confirm in TextEdit:

```text
Archivo → Imprimir → PDF → Guardar como Markdown con MDViewer…
```

Expected: MDViewer opens, Save As appears, cancel leaves no output, success creates GFM and opens it.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock platform/macos/pdf-workflow apps/desktop docs/user-guide/macos-print-workflow.md
git commit -m "feat(macos): install universal Save as Markdown workflow"
```

---

## Task 15: Add Cross-Platform CI and macOS Apple Silicon Release

**Files:**

- Create: `.github/workflows/{ci,release-macos}.yml`
- Create: `scripts/{audit,package-macos-arm64,notarize-macos,verify-release}.sh`
- Create: `docs/release/macos.md`
- Create if absent: `CONTRIBUTING.md`, `SECURITY.md`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`, `README.md`, `.gitignore`
- Modify: legacy release scripts containing machine-specific signing metadata

- [ ] **Step 1: Add a safe local release preflight**

Reject non-Apple-Silicon output, unsigned app/workflow, missing PDFium checksum receipt, hardcoded App Store Connect identifiers, modified lockfiles and a dirty tree.

- [ ] **Step 2: Configure portable CI**

On macOS, Windows and Linux run Rust formatting, clippy, tests, frontend lint/typecheck/tests/build and Tauri smoke build. Cache PDFium by exact release and checksum. Run `cargo audit` and npm audit with reviewed allowlists stored as data.

- [ ] **Step 3: Configure `aarch64-apple-darwin` release only**

Build and sign app plus workflow, bundle pinned PDFium, create DMG, notarize, staple and verify:

```bash
codesign --verify --deep --strict --verbose=2 MDViewer.app
spctl --assess --type execute --verbose=4 MDViewer.app
xcrun stapler validate MDViewer.app
```

Developer ID identity, notary key ID, issuer and private key come only from GitHub Secrets or explicit environment variables.

- [ ] **Step 4: Finish public documentation**

Document macOS 13+, Apple Silicon-only v1, local processing, no OCR until v1.1, fidelity limits, build commands and print-action installation. Link contribution, security and architecture docs.

- [ ] **Step 5: Run release gates without publishing**

```bash
./scripts/verify-workspace.sh
./scripts/audit.sh
./scripts/package-macos-arm64.sh --unsigned-smoke
./scripts/verify-release.sh --unsigned-smoke
```

Expected: workspace and audit pass; unsigned smoke proves architecture and contents while making no signature or notary claim.

- [ ] **Step 6: Commit**

```bash
git add .github .gitignore README.md CONTRIBUTING.md SECURITY.md apps/desktop/src-tauri/tauri.conf.json docs/release scripts legacy/macos-swift/scripts
git commit -m "build: add portable CI and Apple Silicon release pipeline"
```

---

## Task 16: Prove Parity, Tag the Swift Baseline and Retire It

**Files:**

- Create: `docs/release/v1-parity-report.md`
- Create: `tests/parity/manifest.json`
- Create: `scripts/verify-parity.sh`
- Remove after gate: `legacy/macos-swift/`
- Modify: `Cargo.toml`, `README.md`, `scripts/verify-workspace.sh`

- [ ] **Step 1: Create a machine-readable parity manifest**

List every approved v1 behavior and the automated test, manual evidence or explicit exclusion proving it. Include viewer/editor/save/preview/preferences/export, local converters, print integration, cancellation, warnings and cleanup. Record YouTube and OCR as exclusions.

- [ ] **Step 2: Run evidence while Swift still exists**

```bash
./scripts/verify-legacy-swift.sh
./scripts/verify-workspace.sh
./scripts/verify-parity.sh
git status --short
```

Expected: all exit 0; no unreviewed golden diff; only intentionally untracked user files remain.

- [ ] **Step 3: Perform the macOS acceptance matrix**

On macOS 13+ Apple Silicon test Safari, Mail, TextEdit, Preview and available Office apps. Record reading order, headings, lists, tables, links, images, warnings, cancellation and cleanup. Turn every reproducible defect into a fixture before fixing it.

- [ ] **Step 4: Tag the last Swift baseline**

```bash
git tag -a swift-baseline-final -m "Last buildable Swift MDViewer baseline"
git push origin swift-baseline-final
git push onedev swift-baseline-final
```

Expected: both remotes contain the annotated tag before Swift removal.

- [ ] **Step 5: Remove Swift in a dedicated commit**

```bash
git rm -r legacy/macos-swift
./scripts/verify-workspace.sh
./scripts/verify-parity.sh
git add README.md Cargo.toml docs/release/v1-parity-report.md tests/parity scripts
git commit -m "refactor: retire Swift app after verified Tauri parity"
```

- [ ] **Step 6: Build, sign and verify the release candidate**

With release credentials present:

```bash
./scripts/package-macos-arm64.sh
./scripts/notarize-macos.sh
./scripts/verify-release.sh
```

Expected: DMG contains arm64 code only; signatures verify; notarization is stapled; Gatekeeper accepts the app; installed print action completes end to end.

- [ ] **Step 7: Publish only after evidence is attached**

Create the GitHub release from the verified commit and DMG. Push `main` to GitHub first and OneDev second. If OneDev is unavailable, record mirror pending without claiming synchronization.

---

## Final Verification Matrix

| Gate | Command or evidence | Required result |
|---|---|---|
| Core | `cargo test --workspace` | All Rust tests green |
| Rust quality | `cargo fmt`, `cargo clippy -D warnings` | No diff or warnings |
| Frontend | lint, typecheck, Vitest, build | All green |
| Desktop | Playwright smoke on three OSes | Open/edit/save/convert works |
| Local-only | dependency and runtime network checks | No network in conversion path |
| PDF | golden corpus through pinned PDFium | GFM/assets/warnings match |
| Output safety | transaction and collision tests | No partial or foreign deletion |
| macOS integration | real PDF menu invocation | Job → Save As → opened result |
| Privacy/security | path, deep-link, archive and preview tests | Hostile inputs rejected |
| Parity | `scripts/verify-parity.sh` and report | All v1 rows proven or excluded |
| Release | codesign, stapler, `spctl`, architecture | Signed, notarized, arm64 only |
| Repositories | GitHub then OneDev refs | Same published commit and tags |

## Plan Self-Review Gate

Before Task 1, run:

```bash
rg -n 'TB[D]|TO[D]O|FIXM[E]|placeholde[r]|implement late[r]|similar t[o]' docs/superpowers/plans/2026-07-15-cross-platform-save-as-markdown-implementation.md
```

Expected: no matches. Compare this plan against every section of `docs/superpowers/specs/2026-07-15-cross-platform-save-as-markdown-design.md`. A newly discovered product decision changes the spec first, receives review and only then updates this plan.
