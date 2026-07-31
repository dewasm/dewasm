# requires: rt/s32, rt/trap
# `use integer` % is C %: the result takes the dividend's sign, exactly
# wasm's rem_s. INT_MIN % -1 would raise SIGFPE in C; its wasm result is 0.
sub i32_rem_s {
    my $sa = Rt::s32($_[0]);
    my $sb = Rt::s32($_[1]);
    Rt::trap('integer divide by zero') if $sb == 0;
    return 0 if $sb == -1;
    return (do { use integer; $sa % $sb }) & 0xFFFFFFFF;
}
