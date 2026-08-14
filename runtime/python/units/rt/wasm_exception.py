# A wasm exception in flight; the object itself is the exnref value.
# Deliberately unrelated to Trap and Exit: a try_table catches this class alone, so traps and the exit path structurally cannot be caught by catch_all.
class WasmException(Exception):
    def __init__(self, tag, values):
        self.tag = tag
        self.values = values
        super().__init__("wasm exception")
