sub i32_ctz {
    my $x = $_[0];
    return 32 if $x == 0;
    return length(sprintf('%b', $x & (~$x + 1))) - 1;
}
