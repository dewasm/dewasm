def initialize(size)
  @types = Array.new(size)
  @funcs = Array.new(size)
end

def wasm_kind = :table

def size = @funcs.size
