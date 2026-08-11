# requires:
sub wasi_fd_renumber {
    my ($self, $fd, $to) = @_;
    # Both endpoints must be open fds: renumbering onto an unused number is BADF.
    return ERRNO_BADF unless exists $self->{fds}{$fd} && exists $self->{fds}{$to};
    my $dst = $self->{fds}{$to};
    close($dst->{fh}) if !$dst->{dir} && !defined($dst->{std});
    $self->{fds}{$to} = delete $self->{fds}{$fd};
    $self->{meta}{$to} = delete $self->{meta}{$fd};
    return ERRNO_SUCCESS;
}
