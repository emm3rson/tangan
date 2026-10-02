# Release runbook

Use semantic versions: patch for fixes/refactors, minor for compatible tool or
workflow additions, and major for breaking changes.

## Build a release

1. Stage runtime binaries with `npm run prepare:binaries`. Review bundled
   licenses and provenance, including matching FFmpeg source/build provenance
   required for redistribution.
2. Record completed user or operational outcomes in `CHANGELOG.md`. Do not add
   routine check results or implementation narration.
3. Use the build helper to synchronize versions, validate, and package:

   ```powershell
   npm run build:install -- -Bump patch -NoInstall
   # Or choose an explicit version:
   npm run build:install -- -Version X.Y.Z -NoInstall
   ```

   The helper updates `package.json`, `package-lock.json`,
   `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
   `src-tauri/tauri.conf.json`. It runs frontend type/lint/tests and native
   formatting/Clippy/tests. Tauri builds the frontend through its
   `beforeBuildCommand` and packages the per-user NSIS installer under
   `src-tauri/target/release/bundle/nsis/`.
4. Smoke-test the installed application, including export collisions,
   cancellation, persisted settings, and packaged FFmpeg/qpdf/PDF resources.
   Frontend tests and native checks do not establish installed-app acceptance.
5. Review and commit the release manifests, lockfiles, and changelog together.
   Tag or publish only when the release is ready and explicitly authorized.

See [scripts/README.md](../scripts/README.md) for helper flags. To build without
changing versions or launching an installer, use `npm run tauri -- build`.
