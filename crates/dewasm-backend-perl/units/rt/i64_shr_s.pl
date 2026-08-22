# requires: rt/s64
# `use integer` makes >> an arithmetic (sign-extending) shift.
sub i64_shr_s {
    my $sa = Rt::s64($_[0]);
    return (do { use integer; $sa >> ($_[1] & 63) }) & 0xFFFFFFFFFFFFFFFF;
}
