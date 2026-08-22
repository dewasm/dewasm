# requires: rt/s32
# `use integer` makes >> an arithmetic (sign-extending) shift.
sub i32_shr_s {
    my $sa = Rt::s32($_[0]);
    return (do { use integer; $sa >> ($_[1] & 31) }) & 0xFFFFFFFF;
}
