# OpenClaw portable runtime inputs

The release builder accepts only a clean runtime staging directory assembled
from the source-pinned Node archive and OpenClaw tarball. It verifies these
values against `src-tauri/resources/openclaw2-runtime.json` before copying:

| Input | Locked value |
| --- | --- |
| Node | 24.15.0 Windows x64, SHA-256 `cc5149eabd53779ce1e7bdc5401643622d0c7e6800ade18928a767e940bb0e62` |
| OpenClaw | 2026.8.1, SHA-512 integrity `sha512-bSaFeaDFnQH/bU1vgKMac6eHkHHPHG0C/uwduXGI3eIS3lyiYSwmDU5ehhBUUhlPeV85tL5/KVwmoH48nX1tWw==` |
| Clean dependency lock | SHA-256 `649f82e4c452923414a5fd45df645078d0d4ac70599a571ca2f5e7fc0c9e32eb` |
| Upstream workspace writer | SHA-256 `ea0436681164e0dce0d1be6ba57e5bc17a262d412b562ecac98e77a648d8712f` |

The staging process installs dependencies from the normalized lock and does
not copy any existing OpenClaw state, workspace, logs, device wallet, secret,
or runtime application directory.

The builder also verifies the complete staging tree: 37,786 files, SHA-256
`bf78dbf27a3bae3155e53aae49edf548d59e2615b5660ceef260011d4af78003`.
Tree hashes cover ASCII-sorted records of relative path, NUL, hexadecimal
file SHA-256, and LF. Archive hashes alone do not authenticate an expanded tree.

## Rebuilding the compatibility sidecar

Check out `openclaw/fs-safe` at
`524e2a2dd50c390f924a0360c6c71ddf74f70f42`, then run in that checkout:

```sh
git apply --check /path/to/u-king/patches/fs-safe08-windows-compat.patch
git apply /path/to/u-king/patches/fs-safe08-windows-compat.patch
pnpm install --frozen-lockfile
rustup target add wasm32-unknown-unknown
pnpm build
```

Use a separate checkout and dependency directory. On Windows, put the rustup
toolchain before any system Rust installation in PATH. Pass the resulting
`dist` directory to the portable builder; do not edit generated JavaScript.
The verified source rebuild produces 472 files with tree SHA-256
`02761bf6d5c75c3be9d0fea1897eeb81ee884700be373c2ceef4b34c61712e7c`;
`root-impl.js` is
`6f701d377943880178b1478881a613db48e6a949f42a453007301bc61ea9f9fb`.
These are the same bytes used by the NTFS/exFAT compatibility probes.
