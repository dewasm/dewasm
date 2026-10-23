/* Guest-side pieces of the dewasm ruby.h that need no host help beyond the plain imports. */
#include <ruby.h>

int rb_scan_args(int argc, const VALUE *argv, const char *fmt, ...) {
    int n_lead = 0, n_opt = 0, n_trail = 0;
    int f_var = 0, f_hash = 0, f_block = 0;
    const char *p = fmt;
    if (*p >= '0' && *p <= '9') {
        n_lead = *p++ - '0';
        if (*p >= '0' && *p <= '9') n_opt = *p++ - '0';
    }
    if (*p == '*') { f_var = 1; p++; }
    if (*p >= '0' && *p <= '9') n_trail = *p++ - '0';
    if (*p == ':') { f_hash = 1; p++; }
    if (*p == '&') { f_block = 1; p++; }

    VALUE hash = Qnil;
    /* CRuby only takes a keyword-flagged hash; a trailing Hash is the approximation here. */
    if (f_hash && argc > 0 && RB_TYPE_P(argv[argc - 1], T_HASH)) {
        hash = argv[argc - 1];
        argc--;
    }
    int n_mand = n_lead + n_trail;
    if (argc < n_mand || (!f_var && argc > n_mand + n_opt)) {
        rb_raise(rb_eArgError, "wrong number of arguments (given %d, expected %d%s)",
                 argc, n_mand, f_var ? "+" : "");
    }

    va_list ap;
    va_start(ap, fmt);
    int argi = 0;
    for (int i = 0; i < n_lead; i++) {
        VALUE *var = va_arg(ap, VALUE *);
        if (var) *var = argv[argi];
        argi++;
    }
    for (int i = 0; i < n_opt; i++) {
        VALUE *var = va_arg(ap, VALUE *);
        if (argi < argc - n_trail) {
            if (var) *var = argv[argi];
            argi++;
        } else if (var) {
            *var = Qnil;
        }
    }
    if (f_var) {
        int n_var = argc - argi - n_trail;
        VALUE *var = va_arg(ap, VALUE *);
        if (n_var < 0) n_var = 0;
        if (var) *var = n_var > 0 ? rb_ary_new_from_values(n_var, argv + argi) : rb_ary_new();
        argi += n_var;
    }
    for (int i = 0; i < n_trail; i++) {
        VALUE *var = va_arg(ap, VALUE *);
        if (var) *var = argv[argi];
        argi++;
    }
    if (f_hash) {
        VALUE *var = va_arg(ap, VALUE *);
        if (var) *var = hash;
    }
    if (f_block) {
        VALUE *var = va_arg(ap, VALUE *);
        if (var) *var = rb_block_given_p() ? rb_block_proc() : Qnil;
    }
    va_end(ap);
    return argc;
}

/* --- formatting ---
 * CRuby's `rb_raise`, `rb_sprintf`, and friends take `printf` formats plus `PRIsVALUE`.
 * `PRIsVALUE` is `"li" RUBY_PRI_VALUE_MARK`: a `%li` conversion followed by a `\v` byte.
 * Its argument is a VALUE, shown with `to_s`, or with `inspect` when the `+` flag is set.
 * A `vsnprintf` cannot do that, so every conversion is dispatched one by one here.
 */

static size_t dw_put(char *buf, size_t size, size_t pos, const char *s, size_t n) {
    if (pos < size) {
        size_t room = size - pos - 1;
        size_t take = n < room ? n : room;
        memcpy(buf + pos, s, take);
        buf[pos + take] = 0;
    }
    return pos + n;
}

static size_t dw_vsnprintf(char *buf, size_t size, const char *fmt, va_list ap) {
    size_t pos = 0;
    char spec[32], tmp[512];
    if (size) buf[0] = 0;
    while (*fmt) {
        if (*fmt != '%') {
            const char *start = fmt;
            while (*fmt && *fmt != '%') fmt++;
            pos = dw_put(buf, size, pos, start, (size_t)(fmt - start));
            continue;
        }
        const char *start = fmt++;
        int plus = 0, star_w = 0, star_p = 0;
        while (strchr("-+ #0", *fmt) && *fmt) { if (*fmt == '+') plus = 1; fmt++; }
        if (*fmt == '*') { star_w = 1; fmt++; } else while (*fmt >= '0' && *fmt <= '9') fmt++;
        if (*fmt == '.') {
            fmt++;
            if (*fmt == '*') { star_p = 1; fmt++; } else while (*fmt >= '0' && *fmt <= '9') fmt++;
        }
        const char *len = fmt;
        while (strchr("hlLqjzt", *fmt) && *fmt) fmt++;
        size_t len_n = (size_t)(fmt - len);
        char conv = *fmt ? *fmt++ : 0;
        int width = star_w ? va_arg(ap, int) : 0;
        int prec = star_p ? va_arg(ap, int) : 0;
        (void)width; (void)prec;

        if (conv == 'i' && len_n == 1 && *len == 'l' && *fmt == '\v') {
            fmt++;
            VALUE v = va_arg(ap, VALUE);
            VALUE s = plus ? rb_inspect(v) : rb_obj_as_string(v);
            pos = dw_put(buf, size, pos, RSTRING_PTR(s), (size_t)RSTRING_LEN(s));
            continue;
        }
        size_t spec_n = (size_t)(fmt - start);
        if (spec_n >= sizeof spec) spec_n = sizeof spec - 1;
        memcpy(spec, start, spec_n);
        spec[spec_n] = 0;
        int n;
        switch (conv) {
        case '%': n = snprintf(tmp, sizeof tmp, "%%"); break;
        case 'c': case 'd': case 'i': case 'o': case 'u': case 'x': case 'X':
            if (len_n == 2 && len[0] == 'l') n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, long long));
            else if (len_n == 1 && len[0] == 'l') n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, long));
            else if (len_n == 1 && (len[0] == 'z' || len[0] == 't')) n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, size_t));
            else if (len_n == 1 && len[0] == 'j') n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, intmax_t));
            else n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, int));
            break;
        case 'e': case 'E': case 'f': case 'F': case 'g': case 'G': case 'a': case 'A':
            n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, double));
            break;
        case 's': {
            const char *s = va_arg(ap, const char *);
            n = snprintf(tmp, sizeof tmp, spec, s ? s : "(null)");
            break;
        }
        case 'p': n = snprintf(tmp, sizeof tmp, spec, va_arg(ap, void *)); break;
        default: n = snprintf(tmp, sizeof tmp, "%s", spec); break;
        }
        if (n < 0) n = 0;
        if ((size_t)n >= sizeof tmp) n = sizeof tmp - 1;
        pos = dw_put(buf, size, pos, tmp, (size_t)n);
    }
    return pos;
}

static VALUE dw_vformat(const char *fmt, va_list ap) {
    char small[1024];
    va_list ap2;
    va_copy(ap2, ap);
    size_t need = dw_vsnprintf(small, sizeof small, fmt, ap);
    VALUE s;
    if (need < sizeof small) {
        s = rb_utf8_str_new(small, (long)need);
    } else {
        char *big = malloc(need + 1);
        dw_vsnprintf(big, need + 1, fmt, ap2);
        s = rb_utf8_str_new(big, (long)need);
        free(big);
    }
    va_end(ap2);
    return s;
}

VALUE rb_vsprintf(const char *fmt, va_list ap) { return dw_vformat(fmt, ap); }

VALUE rb_sprintf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    return s;
}

VALUE rb_str_catf(VALUE str, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    return rb_str_append(str, s);
}

VALUE rb_exc_new_str_f(VALUE exc, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    return rb_exc_new_str(exc, s);
}

void rb_raise(VALUE exc, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    rb_exc_raise(rb_exc_new_str(exc, s));
    __builtin_unreachable();
}

void rb_warn(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    rb_dw_warn_str(s);
}

void rb_bug(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    rb_dw_bug_str(s);
    __builtin_unreachable();
}

void rb_fatal(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    VALUE s = dw_vformat(fmt, ap);
    va_end(ap);
    rb_dw_bug_str(s);
    __builtin_unreachable();
}

/* `rb_integer_pack` as the sqlite3 gem uses it: one native-order word of 8 bytes, two's complement.
 * The result codes are CRuby's: 0 for zero, +-1 when the value fits, +-2 on overflow.
 */
int rb_integer_pack(VALUE val, void *words, size_t numwords, size_t wordsize, size_t nails, int flags) {
    (void)nails; (void)flags;
    if (numwords != 1 || wordsize != 8) rb_raise(rb_eNotImpError, "rb_integer_pack: only one 64-bit word");
    return rb_dw_int64_pack(val, (long long *)words);
}

/* WASI has no processes, so its C library lacks `getpid`; the `sqlite3` gem records the value. */
pid_t getpid(void) { return 1; }

/* --- `st_table`: open addressing with linear probing, keys compared by the table's type --- */
#include <ruby/st.h>

struct st_table {
    const struct st_hash_type *type;
    st_index_t num_entries;
    st_index_t capacity;
    unsigned char *used;
    st_data_t *keys;
    st_data_t *values;
};

static int st_numcmp(st_data_t a, st_data_t b) { return a != b; }
static st_index_t st_numhash(st_data_t n) { return (n >> 3) * 2654435761u ^ (n >> 19); }
static int st_strcmp_(st_data_t a, st_data_t b) { return strcmp((const char *)a, (const char *)b); }
static st_index_t st_strhash(st_data_t s) {
    const unsigned char *p = (const unsigned char *)s;
    st_index_t h = 5381;
    while (*p) h = h * 33 ^ *p++;
    return h;
}
static const struct st_hash_type st_numtype = { st_numcmp, st_numhash };
static const struct st_hash_type st_strtype = { st_strcmp_, st_strhash };

st_table *st_init_table_with_size(const struct st_hash_type *type, st_index_t size) {
    st_table *t = calloc(1, sizeof *t);
    st_index_t cap = 16;
    while (cap < size * 2) cap *= 2;
    t->type = type;
    t->capacity = cap;
    t->used = calloc(cap, 1);
    t->keys = calloc(cap, sizeof(st_data_t));
    t->values = calloc(cap, sizeof(st_data_t));
    return t;
}
st_table *st_init_table(const struct st_hash_type *type) { return st_init_table_with_size(type, 0); }
st_table *st_init_numtable(void) { return st_init_table_with_size(&st_numtype, 0); }
st_table *st_init_numtable_with_size(st_index_t n) { return st_init_table_with_size(&st_numtype, n); }
st_table *st_init_strtable(void) { return st_init_table_with_size(&st_strtype, 0); }
st_table *st_init_strtable_with_size(st_index_t n) { return st_init_table_with_size(&st_strtype, n); }

void st_free_table(st_table *t) {
    free(t->used);
    free(t->keys);
    free(t->values);
    free(t);
}

static st_index_t st_slot(const st_table *t, st_data_t key, int *found) {
    st_index_t mask = t->capacity - 1;
    st_index_t i = t->type->hash(key) & mask;
    while (t->used[i]) {
        if (t->used[i] == 1 && t->type->compare(t->keys[i], key) == 0) { *found = 1; return i; }
        i = (i + 1) & mask;
    }
    *found = 0;
    return i;
}

static void st_grow(st_table *t) {
    st_table old = *t;
    t->capacity *= 2;
    t->used = calloc(t->capacity, 1);
    t->keys = calloc(t->capacity, sizeof(st_data_t));
    t->values = calloc(t->capacity, sizeof(st_data_t));
    t->num_entries = 0;
    for (st_index_t i = 0; i < old.capacity; i++) {
        if (old.used[i] == 1) st_insert(t, old.keys[i], old.values[i]);
    }
    free(old.used);
    free(old.keys);
    free(old.values);
}

int st_insert(st_table *t, st_data_t key, st_data_t value) {
    int found;
    st_index_t i = st_slot(t, key, &found);
    if (found) { t->values[i] = value; return 1; }
    if ((t->num_entries + 1) * 2 > t->capacity) {
        st_grow(t);
        i = st_slot(t, key, &found);
    }
    t->used[i] = 1;
    t->keys[i] = key;
    t->values[i] = value;
    t->num_entries++;
    return 0;
}

int st_lookup(st_table *t, st_data_t key, st_data_t *value) {
    int found;
    st_index_t i = st_slot(t, key, &found);
    if (found && value) *value = t->values[i];
    return found;
}

int st_delete(st_table *t, st_data_t *key, st_data_t *value) {
    int found;
    st_index_t i = st_slot(t, *key, &found);
    if (!found) { if (value) *value = 0; return 0; }
    if (value) *value = t->values[i];
    *key = t->keys[i];
    t->used[i] = 2; /* a tombstone keeps the probe chain intact */
    t->num_entries--;
    return 1;
}

int st_foreach(st_table *t, int (*func)(st_data_t, st_data_t, st_data_t), st_data_t arg) {
    for (st_index_t i = 0; i < t->capacity; i++) {
        if (t->used[i] != 1) continue;
        switch (func(t->keys[i], t->values[i], arg)) {
        case ST_STOP: return 0;
        case ST_DELETE: t->used[i] = 2; t->num_entries--; break;
        default: break;
        }
    }
    return 0;
}

st_index_t st_table_size(const st_table *t) { return t->num_entries; }
size_t st_memsize(const st_table *t) { return sizeof *t + t->capacity * (1 + 2 * sizeof(st_data_t)); }
