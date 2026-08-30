# One slot per element: a `[type_symbol, callable]` pair for funcref tables, or `nil` for a null slot; a tail-calling function's pair carries its body method as an optional third element (`table/tail_ref`). call_indirect compares type keys, not module-local indices, so a shared table stays consistent across modules.
def initialize(size, max = nil)
  @slots = Array.new(size)
  @max = max
end

def wasm_kind = :table

def size = @slots.size
