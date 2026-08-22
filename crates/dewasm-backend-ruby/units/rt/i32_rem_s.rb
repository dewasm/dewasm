# requires: rt/s32, rt/trap
def i32_rem_s(a, b)
  sa = s32(a)
  sb = s32(b)
  trap("integer divide by zero") if sb == 0
  r = sa.abs % sb.abs
  r = -r if sa < 0
  r & M32
end
