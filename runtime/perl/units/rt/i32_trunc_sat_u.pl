use POSIX ();
sub i32_trunc_sat_u {
    my $x = $_[0];
    return 0 if $x != $x;
    return 0 if $x <= 0.0;
    return 0xFFFFFFFF if $x >= 4294967296.0;
    return POSIX::trunc($x) & 0xFFFFFFFF;
}
