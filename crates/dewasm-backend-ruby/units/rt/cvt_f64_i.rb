# MRI's Integer#to_f rounds to nearest-even, which is exactly the wasm convert semantics.
def cvt_f64_i(v)
  v.to_f
end
