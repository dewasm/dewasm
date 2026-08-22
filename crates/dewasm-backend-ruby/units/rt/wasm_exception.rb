# A wasm exception in flight; the object itself is the exnref value.
# Deliberately unrelated to Trap and Exit: a try_table rescues this class alone, so traps and the exit path structurally cannot be caught by catch_all.
class WasmException < StandardError
  attr_reader :tag, :values

  def initialize(tag, values)
    @tag = tag
    @values = values
    super("wasm exception")
  end
end
