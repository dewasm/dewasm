# requires: rt/trap, rt/wasm_exception
sub throw_ref {
    my ($exn) = @_;
    Rt::trap('null exception reference') unless defined $exn;
    die $exn;
}
