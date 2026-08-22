# requires: rt/s64, rt/trap
# See Rt::i32_div_s: C division via `use integer`, with the INT64_MIN / -1 overflow trapped before it can raise SIGFPE.
sub i64_div_s {
    my $sa = Rt::s64($_[0]);
    my $sb = Rt::s64($_[1]);
    Rt::trap('integer divide by zero') if $sb == 0;
    Rt::trap('integer overflow') if $sa == -9223372036854775808 && $sb == -1;
    return (do { use integer; $sa / $sb }) & 0xFFFFFFFFFFFFFFFF;
}
