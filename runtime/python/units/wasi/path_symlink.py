# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_symlink(self, old_path_ptr, old_path_len, fd, new_path_ptr, new_path_len):
    target = self.memory.read_string(old_path_ptr, old_path_len).decode("utf-8", "surrogateescape")
    # The symlink target is stored verbatim (never pre-resolved); containment is
    # enforced when the link is later followed (ADR-40). An absolute target can
    # never be confined to the preopen, so it is rejected up front.
    if target.startswith("/"):
        return self.ERRNO_NOTCAPABLE
    new_rel = self.memory.read_string(new_path_ptr, new_path_len).decode("utf-8", "surrogateescape")
    link_host, err = self.resolve_path(fd, new_rel, False)
    if err is not None:
        return err
    # A slash-suffixed link name can never be created; the errnos follow
    # wasmtime 47 on both hosts (ADR-49) — EEXIST for an existing directory,
    # ENOTDIR for an existing non-directory, ENOENT when nothing is there —
    # where a raw host symlinkat would diverge (Linux reports EEXIST even for
    # the non-directory shape). The probes use the slash-stripped path.
    if link_host.endswith(os.sep):
        bare = link_host[:-1]
        if os.path.lexists(bare):
            return self.ERRNO_EXIST if os.path.isdir(bare) else self.ERRNO_NOTDIR
        return self.ERRNO_NOENT
    try:
        os.symlink(target, link_host)
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
