# requires: rt/f64_from_bits
# MRI's pack("e")/unpack("e") canonicalize NaNs during the double<->float
# conversion, losing sign and payload. Take a software path for NaNs.
def f32_from_bits(b)
  if (b & 0x7f80_0000) == 0x7f80_0000 && (b & 0x7f_ffff) != 0
    f64_from_bits(((b >> 31) << 63) | 0x7ff0_0000_0000_0000 | ((b & 0x7f_ffff) << 29))
  else
    [b].pack("L<").unpack1("e")
  end
end
