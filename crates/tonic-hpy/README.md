# tonic-hpy

`tonic-hpy` is the isolated adapter boundary for Tonic's HPy Universal host.
The crate implements the pinned HPy 0.9 Universal ABI inventory and the current
H1/H2 host: strict module filename/init-symbol/ABI validation, process-lifetime
library pinning, scoped local handles and VM-owned module methods.

The executable surface is deliberately bounded: `HPy_Dup`/`HPy_Close`, the HPy
0.9 fixed-width/size integer conversions, bool/float conversions, UTF-8 Unicode
conversion and the public exception-state subset.
List/tuple/dict constructors, exact checks, list append and fixed-size list/tuple
builders are also available. `HPyList_New` currently accepts size zero; fixed
sizes use the builder so an uninitialized container never enters the Tonic heap.
`HPyFunc_NOARGS`, `HPyFunc_O`, `HPyFunc_VARARGS` and `HPyFunc_KEYWORDS` module
methods are accepted. Attribute access/mutation, item access/mutation/deletion,
membership, length, repr and positional/keyword calls execute against Tonic
objects without exposing their layout. `HPyErr_SetObject`, built-in exception
hierarchy/tuple matching and `HPyErr_NoMemory` are available; HPy 0.9 has no
public exception fetch/restore API. Custom numeric conversion protocols and
custom exception classes remain unavailable until their protocol/type surfaces.
Per-runtime `HPyGlobal` roots and owner-traced `HPyField` edges are available;
field stores use the generational write barrier and field-only cycles remain
collectible. Native payload embedding, module C state/trackers, types, buffers,
Debug and Trace contexts remain tracked work in the capability manifest and
roadmap.

The official HPy 0.9 header tree used by native integration tests is vendored
under `vendor/hpy-0.9.0`; its source hash and license are recorded there. The
macOS and Linux paths are exercised by real shared-library tests without
`libpython`. The Windows `.hpy0.pyd` path is implemented but remains explicitly
marked as CI-unverified until the H6 platform matrix.
