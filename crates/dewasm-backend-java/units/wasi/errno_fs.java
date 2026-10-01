// File-system-only `errno` codes, kept out of the always-bundled `wasi/_class` prelude.
// So a stdio-only WASI module (no `path_*` / file-system-only `fd_*` imports) doesn't carry them.
// The base codes (BADF/INVAL/IO/SPIPE) live in _class, since stdio needs them too.
static final int WASI_ACCES = 2;
static final int WASI_EXIST = 20;
static final int WASI_ISDIR = 31;
static final int WASI_LOOP = 32;
static final int WASI_NAMETOOLONG = 37;
static final int WASI_NOENT = 44;
static final int WASI_NOTDIR = 54;
static final int WASI_NOTEMPTY = 55;
static final int WASI_PERM = 63;
// WASI_NOTCAPABLE (76) lives in the always-bundled `wasi/_class` prelude.
// The rights model returns it from the stdio-core `fd_*` units too.

// One mapping from a host error to a WASI `errno`, shared by every file system call.
// So the same host error never maps to different codes depending on which system call raised it.
// Java's NIO raises typed subclasses of IOException, so match on those.
// Everything else falls back to EIO.
// Note the gaps compared with the Go/Python backends, which read the raw `errno`.
// Java exposes no distinct exception for EISDIR, ELOOP, or ENAMETOOLONG at open/`stat` time.
// So those host conditions surface as EIO here, unless a system call detects them itself.
// `path_unlink_file`/`path_remove_directory` pre-check for EISDIR/ENOTDIR.
int fs_errno(java.io.IOException e) {
    if (e instanceof java.nio.file.NoSuchFileException) {
        return WASI_NOENT;
    }
    if (e instanceof java.nio.file.FileAlreadyExistsException) {
        return WASI_EXIST;
    }
    if (e instanceof java.nio.file.AccessDeniedException) {
        return WASI_ACCES;
    }
    if (e instanceof java.nio.file.DirectoryNotEmptyException) {
        return WASI_NOTEMPTY;
    }
    if (e instanceof java.nio.file.NotDirectoryException) {
        return WASI_NOTDIR;
    }
    return WASI_IO;
}
