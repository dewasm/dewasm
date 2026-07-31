sub i64_ctz {
    my $x = $_[0];
    return 64 if $x == 0;
    return length(sprintf('%b', $x & (~$x + 1))) - 1;
}
