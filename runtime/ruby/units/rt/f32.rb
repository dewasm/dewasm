# Round a double to single precision. MRI's pack("e") overflows straight
# to infinity for out-of-float-range doubles instead of rounding, so
# values below the rounding boundary (2^128 - 2^103) are mapped back to
# the largest finite f32.
F32_MAX = 3.4028234663852886e+38
F32_OVERFLOW = 2.0**128 - 2.0**103

def f32(x)
  r = [x].pack("e").unpack1("e")
  if r.infinite? && x.finite? && x.abs < F32_OVERFLOW
    r = x < 0 ? -F32_MAX : F32_MAX
  end
  r
end
