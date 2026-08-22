// requires: memory/init, wasi/errno_fs
int wasi_fd_prestat_dir_name(int fd, int pathPtr, int pathLen) {
    Object e = fds.get(fd);
    if (!(e instanceof Dir) || ((Dir) e).preopenName == null) {
        return WASI_BADF;
    }
    byte[] name = ((Dir) e).preopenName;
    if (name.length > Integer.toUnsignedLong(pathLen)) {
        return WASI_NAMETOOLONG;
    }
    memory.init(Integer.toUnsignedLong(pathPtr), name, 0, name.length);
    return WASI_OK;
}
