#include <ruby.h>
#include <ctype.h>

static VALUE cCounter;
static long freed_count = 0;

typedef struct {
    long n;
    VALUE tag;
} counter_t;

static void counter_mark(void *p) {
    counter_t *c = p;
    rb_gc_mark(c->tag);
}

static void counter_free(void *p) {
    freed_count++;
    free(p);
}

static const rb_data_type_t counter_type = {
    "Demo::Counter",
    { counter_mark, counter_free, 0, 0, { 0 } },
    0, 0, RUBY_TYPED_FREE_IMMEDIATELY
};

static VALUE counter_alloc(VALUE klass) {
    counter_t *c = calloc(1, sizeof *c);
    c->tag = Qnil;
    return TypedData_Wrap_Struct(klass, &counter_type, c);
}

static VALUE counter_incr(VALUE self) {
    counter_t *c;
    TypedData_Get_Struct(self, counter_t, &counter_type, c);
    c->n++;
    return LONG2NUM(c->n);
}

static VALUE counter_tag_set(VALUE self, VALUE tag) {
    counter_t *c;
    TypedData_Get_Struct(self, counter_t, &counter_type, c);
    c->tag = tag;
    return tag;
}

static VALUE counter_tag(VALUE self) {
    counter_t *c;
    TypedData_Get_Struct(self, counter_t, &counter_type, c);
    return c->tag;
}

static VALUE demo_add(VALUE self, VALUE a, VALUE b) {
    return LONG2NUM(NUM2LONG(a) + NUM2LONG(b));
}

static VALUE demo_mul64(VALUE self, VALUE v) {
    long long x = NUM2LL(v);
    return LL2NUM(x * 3);
}

static VALUE demo_upcase_bang(VALUE self, VALUE str) {
    char *p = StringValuePtr(str);
    long n = RSTRING_LEN(str);
    for (long i = 0; i < n; i++) p[i] = (char)toupper((unsigned char)p[i]);
    return str;
}

static VALUE demo_concat(VALUE self, VALUE a, VALUE b) {
    long la = RSTRING_LEN(a), lb = RSTRING_LEN(b);
    char *buf = malloc(la + lb);
    memcpy(buf, RSTRING_PTR(a), la);
    memcpy(buf + la, RSTRING_PTR(b), lb);
    VALUE r = rb_str_new(buf, la + lb);
    free(buf);
    return r;
}

static VALUE demo_build(VALUE self, VALUE n) {
    long len = NUM2LONG(n);
    VALUE s = rb_str_buf_new(len);
    char *p = RSTRING_PTR(s);
    for (long i = 0; i < len; i++) p[i] = 'a' + (char)(i % 26);
    rb_str_set_len(s, len);
    return s;
}

static VALUE demo_fail(VALUE self, VALUE msg) {
    char stackbuf[2048];
    memset(stackbuf, 'x', sizeof stackbuf);
    stackbuf[sizeof stackbuf - 1] = 0;
    rb_raise(rb_eArgError, "boom: %s (%d)", StringValueCStr(msg), (int)strlen(stackbuf));
    return Qnil;
}

static VALUE protect_body(VALUE arg) {
    return rb_yield(arg);
}

static VALUE demo_protect(VALUE self, VALUE arg) {
    int state = 0;
    VALUE r = rb_protect(protect_body, arg, &state);
    if (state) {
        VALUE err = rb_errinfo();
        rb_set_errinfo(Qnil);
        return rb_ary_new_from_args(2, INT2FIX(state), err);
    }
    return rb_ary_new_from_args(2, INT2FIX(0), r);
}

static int ensure_ran = 0;
static VALUE ensure_body(VALUE arg) { return rb_yield(arg); }
static VALUE ensure_after(VALUE arg) { ensure_ran++; return Qnil; }

static VALUE demo_ensure(VALUE self, VALUE arg) {
    return rb_ensure(ensure_body, arg, ensure_after, Qnil);
}

static VALUE demo_ensure_count(VALUE self) {
    return INT2NUM(ensure_ran);
}

static VALUE demo_call(VALUE self, VALUE obj, VALUE name) {
    return rb_funcall(obj, rb_intern(StringValueCStr(name)), 0);
}

static VALUE demo_sum(int argc, VALUE *argv, VALUE self) {
    long s = 0;
    for (int i = 0; i < argc; i++) s += NUM2LONG(argv[i]);
    return LONG2NUM(s);
}

static VALUE demo_each_twice(VALUE self, VALUE ary) {
    long n = RARRAY_LEN(ary);
    for (long i = 0; i < n; i++) {
        VALUE v = rb_ary_entry(ary, i);
        rb_yield(v);
        rb_yield(v);
    }
    return Qnil;
}

static VALUE demo_scan(int argc, VALUE *argv, VALUE self) {
    VALUE a, b, rest, opts;
    rb_scan_args(argc, argv, "11*:", &a, &b, &rest, &opts);
    return rb_ary_new_from_args(4, a, b, rest, opts);
}

static VALUE demo_freed_count(VALUE self) {
    return LONG2NUM(freed_count);
}

static VALUE demo_type_name(VALUE self, VALUE v) {
    switch (TYPE(v)) {
    case T_FIXNUM: return rb_str_new_cstr("fixnum");
    case T_BIGNUM: return rb_str_new_cstr("bignum");
    case T_STRING: return rb_str_new_cstr("string");
    case T_ARRAY: return rb_str_new_cstr("array");
    case T_NIL: return rb_str_new_cstr("nil");
    case T_SYMBOL: return rb_str_new_cstr("symbol");
    case T_DATA: return rb_str_new_cstr("data");
    case T_FLOAT: return rb_str_new_cstr("float");
    default: return rb_str_new_cstr("other");
    }
}

static VALUE demo_same(VALUE self, VALUE a, VALUE b) {
    return a == b ? Qtrue : Qfalse;
}

static VALUE demo_float(VALUE self, VALUE v) {
    return DBL2NUM(NUM2DBL(v) * 1.5);
}

void Init_demo(void) {
    VALUE m = rb_define_module("Demo");
    rb_define_module_function(m, "add", demo_add, 2);
    rb_define_module_function(m, "mul64", demo_mul64, 1);
    rb_define_module_function(m, "upcase!", demo_upcase_bang, 1);
    rb_define_module_function(m, "concat", demo_concat, 2);
    rb_define_module_function(m, "build", demo_build, 1);
    rb_define_module_function(m, "fail", demo_fail, 1);
    rb_define_module_function(m, "protect", demo_protect, 1);
    rb_define_module_function(m, "ensure", demo_ensure, 1);
    rb_define_module_function(m, "ensure_count", demo_ensure_count, 0);
    rb_define_module_function(m, "call", demo_call, 2);
    rb_define_module_function(m, "sum", demo_sum, -1);
    rb_define_module_function(m, "each_twice", demo_each_twice, 1);
    rb_define_module_function(m, "scan", demo_scan, -1);
    rb_define_module_function(m, "freed_count", demo_freed_count, 0);
    rb_define_module_function(m, "type_name", demo_type_name, 1);
    rb_define_module_function(m, "same?", demo_same, 2);
    rb_define_module_function(m, "float", demo_float, 1);

    cCounter = rb_define_class_under(m, "Counter", rb_cObject);
    rb_define_alloc_func(cCounter, counter_alloc);
    rb_define_method(cCounter, "incr", counter_incr, 0);
    rb_define_method(cCounter, "tag=", counter_tag_set, 1);
    rb_define_method(cCounter, "tag", counter_tag, 0);
}
