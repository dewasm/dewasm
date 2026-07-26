// requires: wasi/write_string_list
int wasi_environ_get(int environPtr, int bufPtr) {
    return write_string_list(env, environPtr, bufPtr);
}
