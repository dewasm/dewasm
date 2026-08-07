# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_link {
    my ($self, $old_fd, $old_flags, $old_path_ptr, $old_path_len, $new_fd, $new_path_ptr, $new_path_len) = @_;
    # path_link operates on the source link itself; SYMLINK_FOLLOW is
    # rejected rather than silently dereferenced.
    return ERRNO_INVAL if $old_flags & 0x1;
    my $old_rel = $self->{memory}->read_string($old_path_ptr, $old_path_len);
    my ($old_host, $old_err) = $self->resolve_path($old_fd, $old_rel, 0);
    return $old_err if defined $old_err;
    my $new_rel = $self->{memory}->read_string($new_path_ptr, $new_path_len);
    my ($new_host, $new_err) = $self->resolve_path($new_fd, $new_rel, 0);
    return $new_err if defined $new_err;
    # WASI must hardlink the source link itself, but macOS link(2) follows
    # a source symlink (Linux's does not) and core perl has no linkat to
    # ask for NOFOLLOW (Ruby binds it via Fiddle). Clone the symlink
    # instead: same target, though a distinct inode/link count — the
    # closest core-perl emulation. An existing destination still reports
    # EEXIST, as link(2) would.
    if ($^O eq 'darwin' && -l $old_host) {
        my $target = readlink($old_host);
        return $self->fs_errno(0 + $!) unless defined $target;
        symlink($target, $new_host) or return $self->fs_errno(0 + $!);
        return ERRNO_SUCCESS;
    }
    link($old_host, $new_host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
