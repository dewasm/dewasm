# requires: rt/trunc_sat, rt/m64
def i64_trunc_sat_s(x) = m64(trunc_sat(x, -0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff))
