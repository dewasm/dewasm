# requires: rt/trap, rt/u64_trunc
def i64_trunc_u(x)
  trap("invalid conversion to integer") if x.nan?
  trap("integer overflow") if x <= -1.0 || x >= 18446744073709551616.0
  u64_trunc(x)
end
