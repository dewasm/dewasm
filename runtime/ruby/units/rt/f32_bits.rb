# requires: rt/f64_bits, rt/scratch
# The host's double-to-float narrowing quietens a signaling NaN, so a NaN's bits would not survive it.
# Take a software path for NaNs.
def f32_bits(x)
  if x.nan?
    b = f64_bits(x)
    payload = (b >> 29) & 0x7f_ffff
    payload = 0x40_0000 if payload == 0
    (((b >> 63) & 1) << 31) | 0x7f80_0000 | payload
  else
    s = @scratch || scratch
    s.set_value(:f32, 0, x)
    s.get_value(:u32, 0)
  end
end
