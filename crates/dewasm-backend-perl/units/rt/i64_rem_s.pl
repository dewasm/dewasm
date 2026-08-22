# requires: rt/s64, rt/trap
# See Rt::i32_rem_s: C % via `use integer`; INT64_MIN % -1 is 0.
sub i64_rem_s {
    my $sa = Rt::s64($_[0]);
    my $sb = Rt::s64($_[1]);
    Rt::trap('integer divide by zero') if $sb == 0;
    return 0 if $sb == -1;
    return (do { use integer; $sa % $sb }) & 0xFFFFFFFFFFFFFFFF;
}
