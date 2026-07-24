# A wasm exception in flight; the object itself is the exnref value
# (ADR-19). Deliberately not a Trap subclass: traps must never be caught
# by try_table.
class WasmException < StandardError
  attr_reader :tag, :values

  def initialize(tag, values)
    @tag = tag
    @values = values
    super("wasm exception")
  end
end
