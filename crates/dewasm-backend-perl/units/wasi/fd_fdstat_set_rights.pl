# requires:
sub wasi_fd_fdstat_set_rights {
    my ($self, $fd, $fs_rights_base, $fs_rights_inheriting) = @_;
    my $meta = $self->{meta}{$fd};
    return ERRNO_BADF if !defined($meta) || !exists $self->{fds}{$fd};
    # Rights can only be narrowed, never re-granted: a request for any bit the fd does not already hold is NOTCAPABLE.
    if (($fs_rights_base & ~$meta->[0]) || ($fs_rights_inheriting & ~$meta->[1])) {
        return ERRNO_NOTCAPABLE;
    }
    $meta->[0] = $fs_rights_base;
    $meta->[1] = $fs_rights_inheriting;
    return ERRNO_SUCCESS;
}
