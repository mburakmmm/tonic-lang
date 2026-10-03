# tonic-hpy

`tonic-hpy` is the isolated adapter boundary for Tonic's HPy Universal host.
The crate implements H0's pinned ABI inventory and H1a's macOS/Linux loader:
strict module filename, init symbol and ABI validation plus process-lifetime
library pinning.

It does **not** yet create an `HPyContext` or claim that any HPy operation is
executable. H1b adds the minimal local-handle/context surface and end-to-end
constant/Fibonacci modules. Windows loading remains fail-closed.
