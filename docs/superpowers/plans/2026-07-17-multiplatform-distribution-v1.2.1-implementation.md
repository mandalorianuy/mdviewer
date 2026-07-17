# MDViewer v1.2.1 multiplatform distribution implementation plan

1. Add failing release-contract checks for Windows/Linux configs, scripts, workflow, pinned data,
   receipts, signing requirements and version agreement.
2. Add deterministic `tessdata_fast` provisioning and fail-closed AppImage OCR data resolution.
3. Implement Linux AppImage and Debian packaging plus production verification.
4. Implement signed Windows NSIS packaging plus install/launch/uninstall production verification.
5. Add a manually dispatched cross-platform workflow with GitHub artifact attestations.
6. Bump the workspace, desktop and Tauri versions to 1.2.1 and update locks/documentation.
7. Run local governance/full validation, push a PR, require the complete platform CI matrix and
   integrate to both GitHub and OneDev.
8. Run the signed release workflow. If Windows certificate custody is unavailable, stop before tag
   or public release and report that external prerequisite exactly.
