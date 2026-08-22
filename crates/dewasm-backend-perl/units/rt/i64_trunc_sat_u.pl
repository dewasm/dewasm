use POSIX ();
sub i64_trunc_sat_u {
    my $x = $_[0];
    return 0 if $x != $x;
    return 0 if $x <= 0.0;
    return 0xFFFFFFFFFFFFFFFF if $x >= 18446744073709551616.0;
    return POSIX::trunc($x) & 0xFFFFFFFFFFFFFFFF;
}
