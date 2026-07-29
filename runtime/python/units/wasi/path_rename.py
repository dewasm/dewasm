# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_rename(self, old_dirfd, old_path_ptr, old_path_len, new_dirfd, new_path_ptr, new_path_len):
    old_rel = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    # rename(2) never follows trailing symlinks: it moves the link itself and
    # replaces the destination link.
    old_host, err = self.resolve_path(old_dirfd, old_rel, False)
    if err is not None:
        return err
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    new_host, err = self.resolve_path(new_dirfd, new_rel, False)
    if err is not None:
        return err
    # Trailing-slash semantics (issue #42, ADR-49: follow wasmtime): the slash
    # resolve_path preserved lets the host rename(2) enforce the shapes
    # wasmtime inherits from the OS — an existing non-directory behind a slash
    # is ENOTDIR on either side, a missing slash-suffixed source is ENOENT. A
    # *nonexistent* slash-suffixed destination is the one shape wasmtime does
    # not inherit: cap-std strips the slash and the rename proceeds (even from
    # a non-directory source), so strip it here too. The existence probe uses
    # the slash-stripped path: stat on the slash-bearing one already fails
    # ENOTDIR and would misread "exists as a file" as "missing".
    if new_host.endswith(os.sep) and not os.path.lexists(new_host[:-1]):
        new_host = new_host[:-1]
    try:
        os.rename(old_host, new_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
