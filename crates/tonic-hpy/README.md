# tonic-hpy

`tonic-hpy` is the isolated adapter boundary for Tonic's future HPy Universal
host. The crate currently implements milestone H0 only: a pinned ABI inventory,
strict module metadata validation, and a fail-closed capability manifest.

It does **not** load shared libraries, create an `HPyContext`, or claim that any
HPy operation is executable. Those features begin in H1.
