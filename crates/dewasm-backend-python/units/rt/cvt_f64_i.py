# Python's int->float rounds to nearest-even for values within range and raises OverflowError only beyond ~2**1024 (out of i64 range), so plain float() matches the wasm convert semantics for all i32/i64 inputs.
@staticmethod
def cvt_f64_i(v):
    return float(v)
