# requires: rt/link_error
# A present-but-wrong-kind import is a link error, distinct from a missing one (which still falls through to the caller's WASI/ENOSYS/raise fallback via `or`).
# Function values are plain callables; the runtime's own
# Global/Table/Memory wrappers self-report via wasm_kind.
@staticmethod
def check_import_kind(value, kind, mod, name):
    if value is None:
        return value
    if kind == "func":
        ok = callable(value)
    else:
        ok = getattr(value, "wasm_kind", None) == kind
    if ok:
        return value
    raise Rt.LinkError("incompatible import type for %s.%s" % (mod, name))
