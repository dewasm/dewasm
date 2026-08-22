sub popcnt {
    my $b = sprintf('%b', $_[0]);
    return $b =~ tr/1//;
}
