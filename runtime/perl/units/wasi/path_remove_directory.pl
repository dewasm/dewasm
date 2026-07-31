# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_remove_directory {
    my ($self, $dirfd, $path_ptr, $path_len) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    # rmdir(2) never follows a trailing symlink.
    my ($host, $err) = $self->resolve_path($dirfd, $rel, 0);
    return $err if defined $err;
    # rmdir through a trailing slash on an existing directory is EINVAL per
    # wasmtime (ADR-49); other shapes come from the host call.
    return ERRNO_INVAL if substr($host, -1) eq '/' && -d substr($host, 0, -1);
    rmdir($host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
