# MDViewer v1.2.1 multiplatform distribution design

## Outcome

Publish installable MDViewer desktop artifacts for Windows x64 and Linux x64 while preserving the
signed and notarized Apple Silicon macOS distribution. Every public artifact is built from the same
annotated `v1.2.1` tag and carries a commit-bound SHA-256 receipt plus GitHub build provenance.

## Release boundary

- Windows: one per-user NSIS installer named `MDViewer-1.2.1-x64-setup.exe`.
- Linux: one portable AppImage named `MDViewer-1.2.1-x86_64.AppImage` and one Debian package named
  `MDViewer-1.2.1-amd64.deb`.
- macOS: the existing Apple Silicon Developer ID, notarization, stapling and Gatekeeper contract is
  rebuilt for `v1.2.1` without widening its architecture boundary.
- Universal print adapters remain later platform-native distribution work. These desktop packages
  expose Save As conversion but do not claim Windows Print Support App or Linux PAPPL/CUPS support.

## Windows trust and validation

- The release runner imports an Authenticode code-signing certificate from encrypted GitHub
  secrets and provides its SHA-1 thumbprint to Tauri at build time.
- Tauri signs the application executable and NSIS installer using SHA-256 plus a trusted timestamp.
- Packaging fails before producing a publishable receipt if the certificate is missing, invalid or
  unavailable in the current-user certificate store.
- Verification requires `Get-AuthenticodeSignature` to report `Valid` for the installer and the
  installed executable, installs silently for the current user, launches the installed app, and
  uninstalls it without leaving its registry entry.
- CI may compile an unsigned smoke bundle, but an unsigned installer is never a release asset.

## Linux runtime and validation

- Release builds run in Debian 12, the oldest declared binary baseline. The build requires
  Tesseract major version 5, Leptonica, WebKitGTK 4.1 and the normal Tauri desktop dependencies.
- AppImage contains `libtesseract`, Leptonica and pinned `tessdata_fast` 4.1.0 `eng`/`spa` data.
  The data files are downloaded from their immutable tag and verified by SHA-256 before packaging.
- When `APPDIR` is present, OCR resolves only `$APPDIR/usr/share/mdviewer/tessdata`; it does not
  silently fall back to host language data. System and Debian-package runs retain the normal local
  Tesseract data lookup.
- The Debian package declares the Tesseract/Leptonica runtime and language packages explicitly.
- Verification extracts the AppImage, verifies the pinned language hashes and bundled OCR shared
  libraries, inspects Debian dependencies, runs the real Tesseract recall fixture, and launches
  both packaged forms under Xvfb.

## Receipts and provenance

- Platform receipts record schema, version, target, exact Git commit, artifact names and SHA-256.
- Packaging creates receipts with `publishable: false`; only the platform production verifier may
  transition them to `publishable: true`.
- GitHub Actions generates signed build provenance attestations for every public artifact and
  receipt. Release documentation includes `gh attestation verify` instructions.
- Release workflows are manually dispatched and upload short-lived candidates. Publication remains
  a separate authorized step after every platform receipt is publishable.

## Explicit blocker policy

The repository currently has no Windows certificate secrets. Engineering can complete and validate
the packaging path, but the Windows artifact and therefore the public `v1.2.1` release remain
blocked until `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` and
`WINDOWS_CERTIFICATE_THUMBPRINT` are configured with a trusted code-signing certificate.
