# Converts a word read as unsigned to f64 with one rounding.
# A negative word is 2**63 or more: it is halved, with the lost bit kept in the last bit.
# The halved value still has more than 53 bits, so it rounds the way the whole one does.
def cvt_f64_u(v)
  v >= 0 ? v.to_f : (((v >> 1) & 0x7fff_ffff_ffff_ffff) | (v & 1)).to_f * 2.0
end
