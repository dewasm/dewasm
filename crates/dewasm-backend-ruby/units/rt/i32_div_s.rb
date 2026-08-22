# requires: rt/s32, rt/trap
def i32_div_s(a, b)
  sa = s32(a)
  sb = s32(b)
  trap("integer divide by zero") if sb == 0
  q = sa.abs / sb.abs
  q = -q if (sa < 0) ^ (sb < 0)
  trap("integer overflow") if q > 0x7fff_ffff
  q & M32
end
