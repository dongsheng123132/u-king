# OpenClaw portable fs-safe compatibility sidecar

OpenClaw 2026.8.1 keeps its pinned `@openclaw/fs-safe` 0.5.6 unchanged. Its
workspace writer uses a strict Windows identity check which reports a false
path change after a successful replacement on exFAT.

The portable package includes a separate, source-audited fs-safe 0.8.2 sidecar
only for `workspace-fs` when all three conditions hold: Windows, the package
sets `UKING_PORTABLE_COMPAT_EXFAT=1`, and the call explicitly requests
`verify-content-with-lock`. Every other OpenClaw call retains its original
dependency and strict policy.

The sidecar source revision is `524e2a2dd50c390f924a0360c6c71ddf74f70f42`.
The build applies the single dispatch change in
`patches/fs-safe08-windows-compat.patch`; the patched `dist/root-impl.js` must
hash to `6f701d377943880178b1478881a613db48e6a949f42a453007301bc61ea9f9fb`.
The package manifest records every copied sidecar file and the patched
workspace bundle hash. fs-safe is MIT-licensed and its license is shipped in
`LICENSES/fs-safe-MIT.txt`.
