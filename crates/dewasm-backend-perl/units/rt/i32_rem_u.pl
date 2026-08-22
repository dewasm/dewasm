# requires: rt/trap
sub i32_rem_u {
    Rt::trap('integer divide by zero') if $_[1] == 0;
    return $_[0] % $_[1];
}
