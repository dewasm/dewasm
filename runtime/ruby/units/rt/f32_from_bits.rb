# requires: rt/f64_from_bits, rt/scratch
# The host's float-to-double widening quietens a signaling NaN, so a NaN's bits would not survive it.
# Take a software path for NaNs.
def f32_from_bits(b)
  if (b & 0x7f80_0000) == 0x7f80_0000 && (b & 0x7f_ffff) != 0
    f64_from_bits(((b >> 31) << 63) | 0x7ff0_0000_0000_0000 | ((b & 0x7f_ffff) << 29))
  else
    s = @scratch || scratch
    s.set_value(:u32, 0, b)
    s.get_value(:f32, 0)
  end
end
