use POSIX ();
sub i32_trunc_sat_s {
    my $x = $_[0];
    return 0 if $x != $x;
    return 0x80000000 if $x <= -2147483648.0;
    return 0x7FFFFFFF if $x >= 2147483648.0;
    return POSIX::trunc($x) & 0xFFFFFFFF;
}
