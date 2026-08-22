use POSIX ();
sub i64_trunc_sat_s {
    my $x = $_[0];
    return 0 if $x != $x;
    return 0x8000000000000000 if $x <= -9223372036854775808.0;
    return 0x7FFFFFFFFFFFFFFF if $x >= 9223372036854775808.0;
    return POSIX::trunc($x) & 0xFFFFFFFFFFFFFFFF;
}
