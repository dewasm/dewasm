attr_accessor :value

def initialize(value)
  @value = value
end

def wasm_kind = :global
