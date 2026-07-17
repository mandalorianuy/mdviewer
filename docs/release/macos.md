# macOS Apple Silicon release

MDViewer v1 is distributed for macOS 13 or later on Apple Silicon only. The Rust core, CLI and
desktop compile on Windows and Linux in CI, but v1 does not publish binaries for those platforms.

All conversion happens locally. v1 has no OCR, PyTorch, Docling or network conversion. Image-only
and scanned PDFs report that OCR is required; local OCR is planned for v1.1. Printed PDF input can
lose HTML semantics, link structure, form state, accessibility metadata or the original reading
order. Direct HTML remains the higher-fidelity future route for web content.

## PDF print integration and signing boundary

The item at `~/Library/PDF Services/Guardar como Markdown con MDViewer` is a native Finder alias to
`MDViewer.app`. macOS sends the printed PDF to the app as an open-file event. An alias file has no
independent code signature, so the old requirement to sign an executable workflow does not apply to
this approved architecture. Production verification instead proves all of the following:

- the alias lifecycle resolves the exact expected application bundle;
- the outer app has a Developer ID Application signature, TeamIdentifier and hardened runtime;
- bundled `libpdfium.dylib` is signed and the app signature validates inside-out;
- the PDF association has `LSHandlerRank=None`, so MDViewer does not become the default PDF reader.

Installation and removal from the application are documented in the
[macOS print workflow guide](../user-guide/macos-print-workflow.md).

## Local build and unsigned smoke

Install Xcode command-line tools, Node.js 24 and Rust 1.94. Then run:

```bash
npm ci
./scripts/verify-workspace.sh
./scripts/test-release-scripts.sh
./scripts/audit.sh
./scripts/package-macos-arm64.sh --unsigned-smoke
./scripts/verify-release.sh --unsigned-smoke
```

Install the pinned audit tool before `audit.sh` when necessary:

```bash
cargo install cargo-audit --locked --version 0.22.2
```

Unsigned smoke mode may run from a dirty development tree, but states that fact and creates a
receipt with `publishable`, `signed` and `notarized` set to `false`. It validates arm64-only Mach-O
contents, the bundled PDFium path/checksum provenance, PDF open-event metadata and DMG contents. It
does not claim Developer ID signing, alias ownership, notarization, stapling or Gatekeeper approval.
Never publish an unsigned-smoke artifact.

## Production package and notarization

The production scripts accept no embedded identifiers or credential paths. For a local release,
store the Apple notarization credential in Keychain and supply its profile name:

```bash
export CODESIGN_IDENTITY='Developer ID Application: ...'
export APPLE_NOTARY_PROFILE='mdviewer-notary'

./scripts/package-macos-arm64.sh
./scripts/notarize-macos.sh
./scripts/verify-release.sh
```

CI can instead use an App Store Connect API key through these explicit variables:

```bash
export CODESIGN_IDENTITY='Developer ID Application: ...'
export APPLE_API_KEY_PATH='/secure/path/to/AuthKey.p8'
export APPLE_API_KEY='...'
export APPLE_API_ISSUER='...'

./scripts/package-macos-arm64.sh
./scripts/notarize-macos.sh
./scripts/verify-release.sh
```

Choose exactly one notarization method. The preflight rejects a Keychain profile combined with API
key variables, invalid profile names and profiles that `notarytool` cannot validate.

Packaging fails unless the Git tree and both lockfiles are clean. It fetches PDFium from the pinned
Task 7 URL, verifies the archive and receipt, builds only `aarch64-apple-darwin`, signs PDFium before
the outer app, then creates and signs the DMG. Notarization submits the app ZIP with `notarytool`,
staples the app, recreates the DMG with that stapled app, submits the DMG, and staples it. Final
verification runs strict `codesign`, `stapler`, Gatekeeper, architecture/content and isolated native
alias lifecycle gates. The package receipt is bound to the current Git commit and exact executable,
PDFium and DMG checksums before any notary submission. Notarization updates the recreated DMG hash
atomically and leaves `notarized: true`, `publishable: false`. Only `verify-release.sh`, after every
production gate passes, atomically changes `publishable` to `true`; a failed Gatekeeper, mounted-DMG
or alias check leaves it false. Re-running notarization on that pending verified receipt validates
the stapled tickets without submitting the artifacts again.

Verification mounts the DMG read-only and requires its app executable, bundled PDFium, PDF handler
metadata and handler rank to match the exterior app and receipt exactly. Its `Applications` link
must resolve exactly to `/Applications`. Signed mode also revalidates nested and outer signatures,
stapled tickets and Gatekeeper against the mounted copy.

Artifacts are written under `dist/macos-arm64/`. These commands do not create a GitHub release.
The GitHub release workflow uses repository secrets, minimal read-only permissions and uploads a
short-lived workflow artifact for an authorized later publication step.

## CI coverage

`.github/workflows/ci.yml` runs formatting, Clippy, portable unit/layout tests, frontend checks and a
Tauri smoke build on macOS, Windows and Linux. Only the Apple Silicon macOS lane downloads the pinned
PDFium asset and runs extraction/golden tests. RustSec and npm advisory exceptions live as reviewed
data in `config/audit/`. The Rust list records exact transitive build/platform warnings with dated,
scope-specific rationales; npm currently has no exceptions. New advisories fail closed.

See also the [cross-platform design](../superpowers/specs/2026-07-15-cross-platform-save-as-markdown-design.md),
[CLI contract](../reference/cli.md), [contribution guide](../../CONTRIBUTING.md) and
[security policy](../../SECURITY.md).
