# `mdconvert` local conversion CLI

`mdconvert` is the headless, local-only interface to the MDViewer v1 conversion engine. It does
not accept URLs, make network requests, invoke OCR, or shell out to another converter.

## Command

```text
mdconvert convert <INPUT> --output <FILE.md> [--assets <DIR>] [--json] [--cancel-file <PATH>]
```

`INPUT` must be a local regular file, not a symlink. `--output` must name a new lowercase `.md`
file in an existing directory. The CLI refuses an existing Markdown path, existing assets path,
source/output alias, output assets directory that contains the source, unsafe path, or non-regular
input. It never overwrites an existing output.

Every path argument must be Unicode/JSON-representable and is syntax-checked before any filesystem
lookup. UNC, network, device/verbatim/globalroot, drive-relative, foreign-drive, and NTFS alternate
data stream syntax are rejected as `unsafe_path`; platform network-mount prefixes are rejected too.
Windows DOS device aliases such as `CON`, `NUL`, `AUX`, `COM1`, and `LPT1` are rejected
case-insensitively in every component, including extension and trailing-dot/space variants. Option
values cannot be another `--flag`, and duplicate, missing, or unknown options are
`invalid_arguments`. JSON diagnostics are enabled only by a standalone `--json`, never by text
consumed as another option's value.

The input is opened once with no-follow/reparse protection, checked as a regular file, bounded by
the configured input limit, and read once. Detection and conversion use that same owned byte
buffer while the source handle remains open. Unix snapshots include device, inode, length, mtime,
and ctime; Windows holds a sharing mode that denies write/delete and snapshots handle identity,
length, last-write time, and change time. Any change or path replacement fails as `input_changed`;
the CLI never reopens the source for a format-specific converter. Existing output/assets hardlinks
to the source fail as `source_output_alias` before the normal no-clobber check.

The local v1 registry contains PDF, HTML, CSV, JSON, XML, ZIP, EPUB, DOCX, PPTX, XLSX, PNG, and
JPEG. Dispatch is deterministic: strong PDF/PNG/JPEG/ZIP signatures are checked first; structured
JSON/XML/CSV viability is checked second; HTML tokenizer signals are considered last, except that a
compatible explicit `.html` selects authored HTML markup. Thus extensionless JSON or CSV wins even
when strings/cells contain HTML tags, while a PDF renamed `.json` is `format_conflict`. HTML
recognition accepts fragments, custom elements, and comments without a tag allowlist. Unrecognized,
ambiguous, and conflicting inputs fail with `unknown_format`, `ambiguous_format`, and
`format_conflict` respectively. An extensionless ZIP container is intentionally ambiguous because
its package type cannot be selected safely without an explicit `.zip`, `.epub`, `.docx`, `.pptx`,
or `.xlsx` extension.

PDF conversion requires the pinned PDFium library through `PDFIUM_DYNAMIC_LIB_PATH`. An absent or
unloadable runtime fails as `pdfium_unavailable`; encrypted PDFs fail as `encrypted_input`; PDFs
without extractable text fail as `ocr_required`. PNG and JPEG conversion does not run OCR. It can
succeed with an `ocr_deferred` warning.

## Assets and atomic publication

The transactional writer owns a Markdown file and its same-directory assets directory as one
publication. The assets directory is exactly the output path with `.md` replaced by `.assets`:

```text
/work/report.md
/work/report.assets/
```

Omitting `--assets` derives that path. Supplying `--assets` makes the expectation explicit and must
normalize to that exact derived path; arbitrary or cross-directory assets targets are rejected as
`invalid_assets_path`. This restriction keeps Markdown and required assets inside the shared
no-clobber transaction. When the converted document has no assets, only the Markdown file is
published, no directory is created, and `assets_path` is `null`. Required assets are never dropped.

The shared writer stages files on the destination volume, takes an exclusive target lock, rechecks
for races, and commits Markdown plus assets together. Cancellation or failure before commit removes
staging and leaves no partial output.

## JSON result contract

With `--json`, success writes one `mdviewer.convert/v1` envelope to stdout and keeps stderr empty:

```json
{
  "schema_version": "mdviewer.convert/v1",
  "status": "succeeded",
  "markdown_path": "/absolute/normalized/report.md",
  "assets_path": null,
  "metadata": {},
  "warnings": []
}
```

Paths are validated Unicode strings, absolute, and normalized before publication, so success
envelope serialization cannot discover an unrepresentable path after commit. Metadata omits absent
fields. Warnings contain the stable snake-case warning `code`, a `message`, and nullable 1-based
`page`.

Failure writes one envelope to stderr and leaves stdout empty:

```json
{
  "schema_version": "mdviewer.convert/v1",
  "status": "failed",
  "markdown_path": null,
  "assets_path": null,
  "metadata": {},
  "warnings": [],
  "error": {
    "code": "input_not_found",
    "message": "input file does not exist"
  }
}
```

Stable error codes are grouped by boundary:

- Command and path policy: `invalid_arguments`, `unsafe_path`, `invalid_output`,
  `invalid_assets_path`, `source_output_alias`.
- Local input and detection: `input_not_found`, `input_unreadable`, `input_symlink`,
  `input_not_regular`, `input_changed`, `invalid_input`, `unknown_format`, `ambiguous_format`,
  `format_conflict`.
- Conversion: `invalid_request`, `input_io`, `unsupported_format`, `unsupported_input`,
  `corrupt_input`, `encrypted_input`, `limit_exceeded`, `ocr_required`, `pdfium_unavailable`,
  `conversion_failed`.
- Publication and control: `output_exists`, `invalid_output`, `emit_failed`, `output_io`,
  `cancelled`.

Without `--json`, success prints the generated Markdown path to stdout. Warnings and errors go to
stderr. Human diagnostics never print document contents, extracted text, or parser payloads.

## Cancellation

`--cancel-file <PATH>` treats existence of the marker path as a cancellation request. The marker is
checked before input detection, before expensive conversion, after conversion, immediately before
publication, and by the transactional writer before staging and before commit. Cancellation uses
the stable `cancelled` error code, exits with 6, and leaves no Markdown or assets output.

## Exit codes

| Code | Meaning |
| ---: | --- |
| 0 | Conversion and publication succeeded |
| 2 | Invalid command, argument, output path, or assets policy |
| 3 | Missing/invalid local input or unknown, ambiguous, or conflicting format detection |
| 4 | Converter or detection-limit failure, including PDFium, encryption, limits, or OCR requirement |
| 5 | Existing/invalid output or transactional publication failure |
| 6 | Cancellation requested |
