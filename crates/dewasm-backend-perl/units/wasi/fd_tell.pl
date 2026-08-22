# requires: memory/i64_store
sub wasi_fd_tell {
    my ($self, $fd, $out_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_SPIPE if defined $e->{std};
    my $pos = sysseek($e->{fh}, 0, 1);
    return ERRNO_IO unless defined $pos;
    $self->{memory}->i64_store($out_ptr, $pos);
    return ERRNO_SUCCESS;
}
