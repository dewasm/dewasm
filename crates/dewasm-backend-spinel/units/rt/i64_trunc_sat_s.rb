# requires: rt/trunc_sat
# -9223372036854777856.0 is the float next below -2**63.
def i64_trunc_sat_s(x)
  trunc_sat(x, -9223372036854777856.0, 9223372036854775808.0, I64_MIN, 0x7fff_ffff_ffff_ffff)
end
