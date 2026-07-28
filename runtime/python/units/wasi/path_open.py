# requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
def wasi_path_open(self, dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base,
                   fs_rights_inheriting, fdflags, opened_fd_ptr):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    symlink_follow = dirflags & 0x1 != 0  # lookupflags::SYMLINK_FOLLOW
    host_path, err = self.resolve_path(dirfd, rel, symlink_follow)
    if err is not None:
        return err
    # OFLAGS_TRUNC needs the PATH_FILESTAT_SET_SIZE right on the dirfd (ADR-40).
    if oflags & 0x8 != 0 and not (self.fd_meta[dirfd][0] & self.RIGHTS_PATH_FILESTAT_SET_SIZE):
        return self.ERRNO_NOTCAPABLE
    read = fs_rights_base & self.RIGHTS_FD_READ != 0
    write = fs_rights_base & self.RIGHTS_FD_WRITE != 0
    if read and write:
        flags = os.O_RDWR
    elif write:
        flags = os.O_WRONLY
    else:
        flags = os.O_RDONLY
    # Without SYMLINK_FOLLOW a final symlink must not be traversed: O_NOFOLLOW
    # turns that into ELOOP, matching wasmtime's O_NOFOLLOW default.
    if not symlink_follow:
        flags |= os.O_NOFOLLOW
    if oflags & 0x2 != 0:  # oflags::DIRECTORY
        flags |= os.O_DIRECTORY
    if oflags & 0x1 != 0:  # oflags::CREAT
        flags |= os.O_CREAT
    if oflags & 0x4 != 0:  # oflags::EXCL
        flags |= os.O_EXCL
    if oflags & 0x8 != 0:  # oflags::TRUNC
        flags |= os.O_TRUNC
    try:
        fd = os.open(host_path, flags, 0o644)
    except OSError as e:
        return self.fs_errno(e)
    try:
        st = os.fstat(fd)
        if (st.st_mode & 0o170000) == 0o040000:  # a directory
            # Opening a directory: keep a WasiDir (os.fdopen would raise on it),
            # narrow to the directory rights, and drop the throwaway os handle.
            os.close(fd)
            self.fds[self.next_fd] = self.WasiDir(os.path.realpath(host_path), None, None)
            base = fs_rights_base & self.fd_meta[dirfd][1] & self.DIR_RIGHTS_BASE
            inheriting = fs_rights_inheriting & self.fd_meta[dirfd][1] & self.DIR_RIGHTS_INHERITING
        else:
            # Unbuffered (raw) so os.pread/os.pwrite stay coherent with
            # read/write/seek — sqlite mixes both on one fd.
            io_mode = "rb+" if (read and write) else ("wb" if write else "rb")
            self.fds[self.next_fd] = os.fdopen(fd, io_mode, buffering=0)
            base = fs_rights_base & self.fd_meta[dirfd][1] & self.FILE_RIGHTS_BASE
            inheriting = fs_rights_inheriting & self.fd_meta[dirfd][1] & self.FILE_RIGHTS_BASE
    except OSError as e:
        return self.fs_errno(e)
    self.fd_meta[self.next_fd] = [base, inheriting, fdflags & 0xFFFF]
    self.memory.i32_store(opened_fd_ptr, self.next_fd)
    self.next_fd += 1
    return self.ERRNO_SUCCESS
