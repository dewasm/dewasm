# requires: rt/f64_bits
def f32_bits(x)
  if x.nan?
    b = f64_bits(x)
    payload = (b >> 29) & 0x7f_ffff
    payload = 0x40_0000 if payload == 0
    (((b >> 63) & 1) << 31) | 0x7f80_0000 | payload
  else
    [x].pack("e").unpack1("L<")
  end
end
