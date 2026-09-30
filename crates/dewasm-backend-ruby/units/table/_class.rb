# One slot per element.
# It holds a `[type_symbol, callable]` pair for `funcref` tables, or `nil` for a null slot.
# A tail-calling function's pair carries its body method as an optional third element.
# That element is read by `table/tail_ref`.
# `call_indirect` compares type keys, not module-local indices.
# So a shared table stays consistent across modules.
def initialize(size, max = nil)
  @slots = Array.new(size)
  @max = max
end

def wasm_kind = :table

def size = @slots.size
