# requires: rt/s64, rt/trap, rt/m64
def i64_div_s(a, b)
  sa = s64(a)
  sb = s64(b)
  trap("integer divide by zero") if sb == 0
  q = sa.abs / sb.abs
  q = -q if (sa < 0) ^ (sb < 0)
  trap("integer overflow") if q > 0x7fff_ffff_ffff_ffff
  m64(q)
end
