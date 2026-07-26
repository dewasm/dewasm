// requires: rt/exit
int wasi_proc_exit(int code) {
    Rt.exit(code);
    return 0;
}
