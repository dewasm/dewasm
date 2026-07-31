# requires:
sub wasi_fd_close {
    my ($self, $fd) = @_;
    my $e = delete $self->{fds}{$fd};
    return ERRNO_BADF unless defined $e;
    # A dir has no real OS handle to close; stdio is never closed.
    close($e->{fh}) if !$e->{dir} && !defined($e->{std});
    return ERRNO_SUCCESS;
}
