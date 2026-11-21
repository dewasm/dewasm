# requires: rt/u64_trunc
def i64_trunc_sat_u(x)
  return 0 if x.nan? || x <= -1.0
  return M64 if x >= 18446744073709551616.0
  u64_trunc(x)
end
