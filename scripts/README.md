# Build and install

Run the helper with PowerShell 7 (`pwsh`) from the repository root. Stage the
bundled engines first using the setup instructions in [README.md](../README.md).

```powershell
npm run build:install
npm run build:install -- -Bump patch -NoInstall
npm run build:install -- -Version 0.6.0 -NoInstall
npm run build:install:bump
```

| Flag | Effect |
|---|---|
| `-Version X.Y.Z` | Set an explicit version; mutually exclusive with `-Bump`. |
| `-Bump patch\|minor\|major` | Increment the version from `package.json`. |
| `-NoInstall` | Package without launching the installer. |
| `-SkipChecks` | Skip typecheck, lint, tests, formatting, and Clippy. |
| `-NoBuild` | Update versions and lockfiles only. |

Version changes use `npm version` without install scripts or Git tags to keep
both npm manifests synchronized. Cargo updates workspace versions offline while
retaining locked dependencies. Update failures restore the original version
files and stop the script.

Validation runs frontend typecheck, lint, and tests, followed by native
formatting, Clippy, and tests. Tauri builds the frontend through
`beforeBuildCommand` and packages the installer, so the frontend builds once.
Unless `-NoInstall` is passed, the helper launches the newest NSIS installer
(with an MSI fallback if bundle targets change).

See [Build a release](../README.md#build-a-release) for distribution checks and
installer output locations.

Run `npm run test:release` to verify version and lockfile updates in temporary
copies. This requires PowerShell 7 and cached Cargo dependencies; it does not
build or install the app.
