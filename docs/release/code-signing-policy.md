# Code-signing policy

MDViewer publishes platform binaries only when they are bound to the exact source commit and pass
the native release gates documented in this repository. An unsigned Windows installer is never a
release asset.

## Windows

The planned Windows x64 release uses SignPath Foundation's free open-source code-signing service.
The Authenticode private key remains in SignPath's protected signing infrastructure and is not
exported to a maintainer workstation or stored as a GitHub secret. The GitHub Actions trusted build
submits the unsigned candidate and its source provenance to SignPath; the signed result is accepted
only from the configured project and signing policy.

Every production signing request requires manual approval after the approver verifies the release
version, exact Git commit, successful CI build and expected artifact name. The installed executable
and outer NSIS installer must both have valid SHA-256 Authenticode signatures and trusted
timestamps. The release verifier installs, launches and uninstalls the signed package before its
receipt can become publishable.

The certificate publisher is **SignPath Foundation**. If a signing key, account or release is
suspected to be compromised, maintainers stop publication, contact SignPath Foundation, revoke or
reject affected signing requests and disclose the affected versions in a GitHub security advisory.

## macOS and Linux

macOS releases are signed with an Apple Developer ID Application certificate, notarized by Apple,
stapled and accepted by Gatekeeper before publication. Linux AppImage and Debian artifacts are
verified on their target runtime and receive GitHub artifact provenance attestations; their SHA-256
digests are also recorded in commit-bound package receipts.

## Roles and review

- Authors maintain the source, build scripts and pinned dependencies through reviewed Git history.
- Reviewers verify CI, release receipts and the exact candidate commit.
- Approvers perform the manual approval required for a production Windows signing request.

Build automation cannot approve its own signing request. Signing credentials, notarization keys and
private key material must never be committed to the repository or embedded in a release artifact.

Free code signing provided by SignPath.io, certificate by SignPath Foundation
