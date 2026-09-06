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
