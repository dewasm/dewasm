# One slot per element: a `[type_symbol, callable]` pair for funcref
# tables, an arbitrary host value for externref tables, `nil` for null
# (ADR-17). Which kind a table holds is fixed by validation; the runtime
# never needs to distinguish them.
def initialize(size, max = nil)
  @slots = Array.new(size)
  @max = max
end

def wasm_kind = :table

def size = @slots.size
