# Windows and Linux release packaging

## Windows x64

The production workflow requires these encrypted GitHub repository secrets:

- `WINDOWS_CERTIFICATE`: base64-encoded PFX containing a trusted code-signing certificate and its
  private key;
- `WINDOWS_CERTIFICATE_PASSWORD`: PFX export password;
- `WINDOWS_CERTIFICATE_THUMBPRINT`: the certificate's 40-character SHA-1 thumbprint.

The workflow imports the certificate into the ephemeral runner's current-user store. Packaging
fails unless that certificate has a private key and the Code Signing extended-key usage. Tauri
signs both the application and NSIS installer with SHA-256 and a trusted timestamp.

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
