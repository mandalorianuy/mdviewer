# Windows and Linux release packaging

## Windows x64

The current production workflow has a fail-closed PFX signing adapter but no certificate is stored
in GitHub. MDViewer is applying to SignPath Foundation's free open-source program so that the
Windows Authenticode key remains in protected signing infrastructure instead of being exported to a
repository secret. The [code-signing policy](code-signing-policy.md) defines custody, approval and
incident handling.

After SignPath accepts the project, the workflow will build the unsigned candidate on a GitHub-hosted
runner, submit its trusted-build provenance for manual approval, retrieve the signed result and then
run the existing production verifier. The legacy PFX adapter will be removed only after the
SignPath path proves both the application and NSIS installer signatures end to end.

`verify-windows-release.ps1` then requires valid Authenticode on the installer and installed
executable, performs a silent per-user install, launches the application, uninstalls it, and only
then marks the receipt publishable. Unsigned NSIS output is never a release asset.

## Linux x64

The production job runs inside Debian 12 and requires Tesseract major version 5. It builds an
AppImage and Debian package with:

```bash
./scripts/package-linux-x64.sh
./scripts/verify-linux-release.sh
```

`fetch-tessdata.sh` downloads `eng` and `spa` from the immutable `tessdata_fast` 4.1.0 tag and
checks their pinned SHA-256 values. The verifier extracts the AppImage, checks both language files
and the bundled Tesseract/Leptonica libraries, inspects Debian dependencies, runs the real OCR
fixture, and launches each packaged form under Xvfb.

## Candidate workflow

Run `Release Windows and Linux` manually in GitHub Actions. The `platform` input accepts `all`,
`windows`, or `linux`, so each platform can be proven independently while preserving `all` as the
final release gate. It never triggers automatically from a tag. Each verified candidate receives a
GitHub/Sigstore provenance attestation and is retained as a workflow artifact for 14 days. Release
publication is a distinct authorized step.
