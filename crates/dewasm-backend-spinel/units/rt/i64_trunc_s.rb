# requires: rt/trunc_checked
# -9223372036854777856.0 is the float next below -2**63.
def i64_trunc_s(x) = trunc_checked(x, -9223372036854777856.0, 9223372036854775808.0)
