# requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
def wasi_path_open(self, dirfd, dirflags, path_ptr, path_len, oflags, fs_rights_base,
                   fs_rights_inheriting, fdflags, opened_fd_ptr):
    rel = self.memory.read_string(path_ptr, path_len).decode("utf-8", "surrogateescape")
    host_path, err = self.resolve_path(dirfd, rel)
    if err is not None:
        return err
    try:
        if oflags & 0x2 != 0:  # oflags::DIRECTORY
            # os.path.isdir is false for both "missing" and "not a directory";
            # POSIX O_DIRECTORY distinguishes them (ENOENT vs ENOTDIR).
            if not os.path.exists(host_path):
                return self.ERRNO_NOENT
            if not os.path.isdir(host_path):
                return self.ERRNO_NOTDIR
            self.fds[self.next_fd] = self.WasiDir(host_path, None, None)
        else:
            read = fs_rights_base & 0x2 != 0   # rights::FD_READ
            write = fs_rights_base & 0x40 != 0  # rights::FD_WRITE
            if read and write:
                flags = os.O_RDWR
            elif write:
                flags = os.O_WRONLY
            else:
                flags = os.O_RDONLY
            if oflags & 0x1 != 0:  # oflags::CREAT
                flags |= os.O_CREAT
            if oflags & 0x4 != 0:  # oflags::EXCL
                flags |= os.O_EXCL
            if oflags & 0x8 != 0:  # oflags::TRUNC
                flags |= os.O_TRUNC
            if fdflags & 0x1 != 0:  # fdflags::APPEND
                flags |= os.O_APPEND
            fd = os.open(host_path, flags, 0o644)
            # Unbuffered (raw) so os.pread/os.pwrite stay coherent with
            # read/write/seek — sqlite mixes both on one fd.
            io_mode = "rb+" if (read and write) else ("wb" if write else "rb")
            self.fds[self.next_fd] = os.fdopen(fd, io_mode, buffering=0)
        self.memory.i32_store(opened_fd_ptr, self.next_fd)
        self.next_fd += 1
    except OSError as e:
        return self.fs_errno(e)
    return self.ERRNO_SUCCESS
