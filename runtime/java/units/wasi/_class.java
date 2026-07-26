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
    final boolean append;

    Handle(java.nio.channels.FileChannel ch, java.nio.file.Path path, boolean append) {
        this.ch = ch;
        this.path = path;
        this.append = append;
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
