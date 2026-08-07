# requires:
sub wasi_fd_renumber {
    my ($self, $fd, $to) = @_;
    # Both descriptors must currently be open; the destination's own
    # resource is closed, then the source is moved onto it and the source
    # number retired (works onto stdio and preopens too).
    return ERRNO_BADF unless exists $self->{fds}{$fd} && exists $self->{fds}{$to};
    my $dst = $self->{fds}{$to};
    close($dst->{fh}) if !$dst->{dir} && !defined($dst->{std});
    $self->{fds}{$to} = delete $self->{fds}{$fd};
    $self->{meta}{$to} = delete $self->{meta}{$fd};
    return ERRNO_SUCCESS;
}
