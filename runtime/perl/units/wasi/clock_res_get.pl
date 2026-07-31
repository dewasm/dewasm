# requires: memory/i64_store
sub wasi_clock_res_get {
    my ($self, $id, $out_ptr) = @_;
    $self->{memory}->i64_store($out_ptr, 1);
    return ERRNO_SUCCESS;
}
