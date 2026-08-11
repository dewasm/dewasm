# requires: rt/f64_bits, rt/f64_from_bits
# Quiet a NaN (set the quiet bit), preserving sign and payload.
# The f64 quiet bit maps to the f32 quiet bit for NaNs that came from f32.
def quiet_nan(x)
  f64_from_bits(f64_bits(x) | 0x0008_0000_0000_0000)
end
