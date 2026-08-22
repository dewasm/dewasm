# requires: rt/s32, rt/trap
# `use integer` division is C division: truncating, exactly wasm's div_s.
# The INT_MIN / -1 overflow would raise SIGFPE in C, so trap it first.
sub i32_div_s {
    my $sa = Rt::s32($_[0]);
    my $sb = Rt::s32($_[1]);
    Rt::trap('integer divide by zero') if $sb == 0;
    Rt::trap('integer overflow') if $sa == -2147483648 && $sb == -1;
    return (do { use integer; $sa / $sb }) & 0xFFFFFFFF;
}
