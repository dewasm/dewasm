// Filesystem-only errno codes: kept out of the always-bundled
// wasi/_class prelude so a stdio-only WASI module (no path_* / fs-only fd_*
// imports) doesn't carry them. The base codes (BADF/INVAL/IO/SPIPE) live in
// _class since stdio needs them too.
static final int WASI_ACCES = 2;
static final int WASI_EXIST = 20;
static final int WASI_ISDIR = 31;
static final int WASI_LOOP = 32;
static final int WASI_NAMETOOLONG = 37;
static final int WASI_NOENT = 44;
static final int WASI_NOTDIR = 54;
static final int WASI_NOTEMPTY = 55;
static final int WASI_PERM = 63;
// WASI_NOTCAPABLE (76) lives in the always-bundled wasi/_class prelude, since
// the rights model enforces it from stdio-core fd_* units too.

// One host-error-to-WASI-errno mapping shared by every filesystem syscall, so
// the same host error never maps to different codes depending on which syscall
// raised it. Java's NIO raises typed subclasses of IOException, so match on
// those; everything else falls back to EIO. Note the honest gaps vs
// the Go/Python backends, which read the raw errno: Java exposes no distinct
// exception for EISDIR, ELOOP, or ENAMETOOLONG at open/stat time, so those
// host conditions surface as EIO here unless a syscall detects them itself
// (path_unlink_file/path_remove_directory pre-check for EISDIR/ENOTDIR).
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
