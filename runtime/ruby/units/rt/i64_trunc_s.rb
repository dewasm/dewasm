# requires: rt/trunc_checked
def i64_trunc_s(x) = trunc_checked(x, -0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff) & M64
