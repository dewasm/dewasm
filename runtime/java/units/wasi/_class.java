// requires: memory/_class
// The bundled WASI preview 1 runtime (ADR-30; filesystem model ADR-14, adopted
// one-for-one from the Go/Python/Ruby backends). The fd table holds one of:
// an `java.io.InputStream`/`java.io.OutputStream` for the inherited standard
// streams (fds 0/1/2), a `Handle` (a guest-opened regular file over a
// seekable `FileChannel`), or a `Dir` (a preopen or a directory the guest
// opened via path_open). Args/env are pre-encoded UTF-8 byte strings; env is
// passed already-ordered ("K=V") and preopens are assigned fds in sorted
// order, so there is no map-iteration nondeterminism (ADR-30). Stdio is
// byte-wise (raw streams, not the text PrintStream) so output is
// byte-identical.
static final int WASI_OK = 0;
static final int WASI_BADF = 8;
static final int WASI_INVAL = 28;
static final int WASI_IO = 29;
static final int WASI_NOSYS = 52;
static final int WASI_SPIPE = 70;
// NOTCAPABLE lives in the always-bundled prelude (not errno_fs) because the
// per-fd rights model (ADR-40) enforces it from core fd_read/fd_write/fd_seek
// too, not only from the path_* units that pull in errno_fs.
static final int WASI_NOTCAPABLE = 76;

// WASI p1 rights bits (ADR-40). Access in this runtime is "which directories
// did the embedder preopen" (ADR-14) plus a capability-narrowing rights model
// on top: a path_open grants requested & dir.inheriting masked by the opened
// fd's filetype, fd_fdstat_set_rights can only narrow further, and the
// enforced syscalls (fd_read/write/seek/readdir, fd_filestat_set_size,
// path_open) reject a missing right with NOTCAPABLE.
static final long R_FD_DATASYNC = 1L << 0;
static final long R_FD_READ = 1L << 1;
static final long R_FD_SEEK = 1L << 2;
static final long R_FD_FDSTAT_SET_FLAGS = 1L << 3;
static final long R_FD_SYNC = 1L << 4;
static final long R_FD_TELL = 1L << 5;
static final long R_FD_WRITE = 1L << 6;
static final long R_FD_ADVISE = 1L << 7;
static final long R_FD_ALLOCATE = 1L << 8;
static final long R_PATH_CREATE_DIRECTORY = 1L << 9;
static final long R_PATH_CREATE_FILE = 1L << 10;
static final long R_PATH_LINK_SOURCE = 1L << 11;
static final long R_PATH_LINK_TARGET = 1L << 12;
static final long R_PATH_OPEN = 1L << 13;
static final long R_FD_READDIR = 1L << 14;
static final long R_PATH_READLINK = 1L << 15;
static final long R_PATH_RENAME_SOURCE = 1L << 16;
static final long R_PATH_RENAME_TARGET = 1L << 17;
static final long R_PATH_FILESTAT_GET = 1L << 18;
static final long R_PATH_FILESTAT_SET_SIZE = 1L << 19;
static final long R_PATH_FILESTAT_SET_TIMES = 1L << 20;
static final long R_FD_FILESTAT_GET = 1L << 21;
static final long R_FD_FILESTAT_SET_SIZE = 1L << 22;
static final long R_FD_FILESTAT_SET_TIMES = 1L << 23;
static final long R_PATH_SYMLINK = 1L << 24;
static final long R_PATH_REMOVE_DIRECTORY = 1L << 25;
static final long R_PATH_UNLINK_FILE = 1L << 26;
static final long R_POLL_FD_READWRITE = 1L << 27;

// The rights a directory fd may hold (base) and the file/dir rights it may
// hand down (inheriting), mirroring wasmtime's DIR_RIGHTS / FILE_RIGHTS masks
// so the hard-coded expectations in the wasi-testsuite (path_open_preopen's
// directory_base_rights / directory_inheriting_rights) are met and a directory
// never reports file-only rights like FD_FILESTAT_SET_SIZE.
static final long DIR_RIGHTS = R_FD_FDSTAT_SET_FLAGS | R_FD_SYNC | R_PATH_CREATE_DIRECTORY
    | R_PATH_CREATE_FILE | R_PATH_LINK_SOURCE | R_PATH_LINK_TARGET | R_PATH_OPEN | R_FD_READDIR
    | R_PATH_READLINK | R_PATH_RENAME_SOURCE | R_PATH_RENAME_TARGET | R_PATH_FILESTAT_GET
    | R_PATH_FILESTAT_SET_SIZE | R_PATH_FILESTAT_SET_TIMES | R_FD_FILESTAT_GET
    | R_FD_FILESTAT_SET_TIMES | R_PATH_SYMLINK | R_PATH_REMOVE_DIRECTORY | R_PATH_UNLINK_FILE
    | R_POLL_FD_READWRITE;
static final long FILE_RIGHTS = R_FD_DATASYNC | R_FD_READ | R_FD_SEEK | R_FD_FDSTAT_SET_FLAGS
    | R_FD_SYNC | R_FD_TELL | R_FD_WRITE | R_FD_ADVISE | R_FD_ALLOCATE | R_FD_FILESTAT_GET
    | R_FD_FILESTAT_SET_SIZE | R_FD_FILESTAT_SET_TIMES | R_POLL_FD_READWRITE;

// A directory descriptor (ADR-14): either a preopen (preopenName set to the
// guest-visible path passed in preopens) or a directory the guest opened
// itself via path_open (preopenName null). entries is the fd_readdir listing
// cache, filled lazily; loaded guards the one-shot snapshot.
static final class Dir {
    final java.nio.file.Path hostPath;
    final byte[] preopenName;
    java.util.List<Dirent> entries;
    boolean loaded;

    Dir(java.nio.file.Path hostPath, byte[] preopenName) {
        this.hostPath = hostPath;
        this.preopenName = preopenName;
    }
}

static final class Dirent {
    final byte[] name;
    final byte filetype;

    Dirent(byte[] name, byte filetype) {
        this.name = name;
        this.filetype = filetype;
    }
}

// A guest-opened regular file: one seekable FileChannel gives coherent
// read/write/seek/tell plus positional pread/pwrite (ADR-14). `path` is kept
// for fd_filestat_get; `append` reproduces O_APPEND by seeking to end before
// each write.
static final class Handle {
    final java.nio.channels.FileChannel ch;
    final java.nio.file.Path path;
    // Mutable: fd_fdstat_set_flags can toggle O_APPEND after open (ADR-40).
    boolean append;

    Handle(java.nio.channels.FileChannel ch, java.nio.file.Path path, boolean append) {
        this.ch = ch;
        this.path = path;
        this.append = append;
    }
}

// The per-fd capability state parallel to the fds table (ADR-40): the granted
// base/inheriting rights and the open fdflags. Stdio fds carry no FdMeta and
// are treated as fully capable; every path_open'd fd and every preopen gets
// one.
static final class FdMeta {
    long base;
    long inheriting;
    int fdflags;

    FdMeta(long base, long inheriting, int fdflags) {
        this.base = base;
        this.inheriting = inheriting;
        this.fdflags = fdflags;
    }
}

// A sandbox path resolution result: `path` (the confined host path) is valid
// only when `errno == WASI_OK`. Java has no tuples, so resolve_path returns
// this instead of Go's (string, errno) pair (ADR-30).
static final class Resolved {
    final String path;
    final int errno;

    Resolved(String path, int errno) {
        this.path = path;
        this.errno = errno;
    }
}

byte[][] args;
byte[][] env;
Memory memory;
java.util.Map<Integer, Object> fds = new java.util.HashMap<>();
java.util.Map<Integer, FdMeta> meta = new java.util.HashMap<>();
int nextFd;
java.io.OutputStream stdout = new java.io.FileOutputStream(java.io.FileDescriptor.out);
java.io.OutputStream stderr = new java.io.FileOutputStream(java.io.FileDescriptor.err);
java.io.InputStream stdin = System.in;
java.security.SecureRandom rng = new java.security.SecureRandom();

WASI(String[] args, String[] env, java.util.Map<String, String> preopens) {
    this.args = encode(args);
    this.env = encode(env);
    this.fds.put(0, stdin);
    this.fds.put(1, stdout);
    this.fds.put(2, stderr);
    int next = 3;
    if (preopens != null) {
        // Assign preopen fds in sorted guest-path order for determinism.
        java.util.List<String> guests = new java.util.ArrayList<>(preopens.keySet());
        java.util.Collections.sort(guests);
        for (String guest : guests) {
            java.nio.file.Path real = java.nio.file.Paths.get(preopens.get(guest)).toAbsolutePath();
            try {
                real = real.toRealPath();
            } catch (java.io.IOException e) {
                // Leave `real` as the absolute path; the isDirectory check below
                // still rejects a non-directory or missing preopen.
            }
            if (!java.nio.file.Files.isDirectory(real)) {
                throw new RuntimeException(
                    "preopen " + guest + " => " + preopens.get(guest) + ": not a directory");
            }
            this.fds.put(next, new Dir(real, guest.getBytes(java.nio.charset.StandardCharsets.UTF_8)));
            // A preopen holds every directory right and hands down every file
            // right (ADR-40): full authority within the embedder-authorized
            // tree, narrowed only as the guest opens paths through it.
            this.meta.put(next, new FdMeta(DIR_RIGHTS, DIR_RIGHTS | FILE_RIGHTS, 0));
            next++;
        }
    }
    this.nextFd = next;
}

private static byte[][] encode(String[] xs) {
    if (xs == null) {
        xs = new String[0];
    }
    byte[][] out = new byte[xs.length][];
    for (int i = 0; i < xs.length; i++) {
        out[i] = xs[i].getBytes(java.nio.charset.StandardCharsets.UTF_8);
    }
    return out;
}

// Whether the fd table entry is one of the three inherited standard streams,
// which take the SPIPE/no-close special cases (in lockstep with fds 0..2).
private static boolean isStdio(Object entry) {
    return entry instanceof java.io.InputStream || entry instanceof java.io.OutputStream;
}

// True when fd carries a rights meta that does not grant `need`. An fd with no
// meta (the inherited stdio streams) is treated as fully capable, so this only
// gates the path_open'd/preopen fds the rights model actually tracks (ADR-40).
boolean lacksRight(int fd, long need) {
    FdMeta m = meta.get(fd);
    return m != null && (m.base & need) == 0;
}
