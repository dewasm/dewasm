# One boxed global: a shared mutable cell so a global that crosses an
# instantiation boundary (imported, or exported and later imported by
# another instance) stays shared, not copied (ADR-16). Memory/Table are
# already objects for the same reason; boxing Global keeps one
# representation for every global read/write/export site. `value` is a
# plain attribute.
wasm_kind = "global"

def __init__(self, value):
    self.value = value
