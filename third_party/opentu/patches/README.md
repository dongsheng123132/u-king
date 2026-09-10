# U-King OpenTu patches

Patches in this directory must be plain `git apply --check`-able diffs against
the commit pinned in `../lock.json`. The build script refuses any patch whose
preimage does not apply. The first integration deliberately carries no UI patch:
the Rust project/action boundary is landed and tested before changing upstream
canvas internals.

When the bridge is added, it may expose only `project.load`, `project.save`,
and `image.submit`; it must not read keys, persist capabilities, or proxy URLs.
