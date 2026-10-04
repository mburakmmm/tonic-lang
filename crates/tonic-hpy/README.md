# tonic-hpy

`tonic-hpy` is the isolated adapter boundary for Tonic's HPy Universal host.
The crate implements the pinned HPy 0.9 Universal ABI inventory and H1 host:
strict module filename/init-symbol/ABI validation, process-lifetime library
pinning, a minimal context, local handles and VM-owned module methods.

The executable surface is deliberately small: `HPy_Dup`/`HPy_Close`, signed
64-bit integer conversion, UTF-8 Unicode conversion and basic exception state.
Only `HPyFunc_NOARGS` and `HPyFunc_O` module methods are accepted. Containers,
attributes, calls, globals, fields, types, buffers, Debug and Trace contexts
remain unavailable and are tracked by the capability manifest.

The official HPy 0.9 header tree used by native integration tests is vendored
under `vendor/hpy-0.9.0`; its source hash and license are recorded there. The
macOS and Linux paths are exercised by real shared-library tests without
`libpython`. The Windows `.hpy0.pyd` path is implemented but remains explicitly
marked as CI-unverified until the H6 platform matrix.
