# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_rename {
    my ($self, $old_dirfd, $old_path_ptr, $old_path_len, $new_dirfd, $new_path_ptr, $new_path_len) = @_;
    my $old_rel = $self->{memory}->read_string($old_path_ptr, $old_path_len);
    # rename(2) never follows trailing symlinks: it moves the link itself
    # and replaces the destination link.
    my ($old_host, $old_err) = $self->resolve_path($old_dirfd, $old_rel, 0);
    return $old_err if defined $old_err;
    my $new_rel = $self->{memory}->read_string($new_path_ptr, $new_path_len);
    my ($new_host, $new_err) = $self->resolve_path($new_dirfd, $new_rel, 0);
    return $new_err if defined $new_err;
    # The preserved slash lets the host rename(2) enforce the existing and
    # missing shapes; a *nonexistent* slash-suffixed destination is
    # stripped so the rename proceeds, as wasmtime does (issue #42). Probe
    # the bare path — stat on "x/" fails ENOTDIR and reads as missing.
    if (substr($new_host, -1) eq '/' && !lstat(substr($new_host, 0, -1))) {
        $new_host = substr($new_host, 0, -1);
    }
    rename($old_host, $new_host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
