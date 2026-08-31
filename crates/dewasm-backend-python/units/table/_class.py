# One slot per element: a [type_key, callable] pair for funcref tables, or
# None for a null slot; a tail-calling function's pair carries its body method as an optional third element (table/tail_ref). call_indirect compares type keys, not module-local indices, so a shared table stays consistent across modules.
wasm_kind = "table"

def __init__(self, size, max=None):
    self._slots = [None] * size
    self._max = max

def size(self):
    return len(self._slots)
