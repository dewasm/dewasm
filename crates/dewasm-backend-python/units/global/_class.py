# One boxed global: a shared mutable cell.
# A global that crosses an instantiation boundary then stays shared, not copied.
# It crosses one when imported, or when exported and later imported by another instance.
# Memory/Table are already objects for the same reason.
# Boxing Global keeps one representation for every global read/write/export site.
# `value` is a plain attribute.
wasm_kind = "global"

def __init__(self, value):
    self.value = value
