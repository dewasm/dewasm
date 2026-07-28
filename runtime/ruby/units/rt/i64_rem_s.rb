# requires: rt/s64, rt/trap, rt/m64
def i64_rem_s(a, b)
  sa = s64(a)
  sb = s64(b)
  trap("integer divide by zero") if sb == 0
  r = sa.abs % sb.abs
  r = -r if sa < 0
  m64(r)
end
