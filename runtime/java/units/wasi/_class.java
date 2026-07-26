// requires: memory/_class
// The bundled WASI preview 1 core (ADR-30): the eight syscalls cowsay imports
// — args/env (args_get/args_sizes_get/environ_get/environ_sizes_get), fd_read,
// fd_write, proc_exit, random_get — over an fd table where 0/1/2 are
// stdin/stdout/stderr. No filesystem in milestone 1 (ADR-24). Args/env are
// pre-encoded UTF-8 byte strings; stdio is byte-wise (raw streams, not the
// text PrintStream) so output is byte-identical.
static final int WASI_OK = 0;
static final int WASI_BADF = 8;
static final int WASI_INVAL = 28;
static final int WASI_IO = 29;
static final int WASI_NOSYS = 52;

byte[][] args;
byte[][] env;
Memory memory;
java.io.OutputStream stdout = new java.io.FileOutputStream(java.io.FileDescriptor.out);
java.io.OutputStream stderr = new java.io.FileOutputStream(java.io.FileDescriptor.err);
java.io.InputStream stdin = System.in;
java.security.SecureRandom rng = new java.security.SecureRandom();

WASI(String[] args, String[] env) {
    this.args = encode(args);
    this.env = encode(env);
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
