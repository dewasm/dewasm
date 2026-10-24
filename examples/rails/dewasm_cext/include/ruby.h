#ifndef DEWASM_RUBY_H
#define DEWASM_RUBY_H

#include <stddef.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <sys/types.h>
#include <time.h>
#include <ruby/st.h>
#include <unistd.h>

typedef unsigned long VALUE;
typedef unsigned long ID;
#define ANYARGS

#define RB_IMPORT(name) __attribute__((import_module("ruby"), import_name(#name)))
#define RB_NORETURN __attribute__((noreturn))

#define Qfalse ((VALUE)0x00)
#define Qtrue  ((VALUE)0x02)
#define Qnil   ((VALUE)0x04)
#define Qundef ((VALUE)0x06)

#define RTEST(v) (((VALUE)(v) & ~Qnil) != 0)
#define NIL_P(v) ((VALUE)(v) == Qnil)
#define RB_TEST(v) RTEST(v)
#define RB_NIL_P(v) NIL_P(v)
#define FIXNUM_P(v) (((VALUE)(v) & 1) != 0)
#define SPECIAL_CONST_P(v) (FIXNUM_P(v) || (VALUE)(v) <= Qundef)
#define UNDEF_P(v) ((VALUE)(v) == Qundef)

#define INT2FIX(i) (((VALUE)((long)(i)) << 1) | 1)
#define LONG2FIX(i) INT2FIX(i)
#define FIX2LONG(v) ((long)(v) >> 1)
#define FIX2INT(v) ((int)FIX2LONG(v))
#define FIX2ULONG(v) ((unsigned long)FIX2LONG(v))
#define FIXNUM_MAX (0x3fffffffL)
#define FIXNUM_MIN (-0x40000000L)
#define FIXABLE(i) ((long long)(i) <= FIXNUM_MAX && (long long)(i) >= FIXNUM_MIN)
#define POSFIXABLE(i) ((unsigned long long)(i) <= (unsigned long long)FIXNUM_MAX)

/* The host creates these handles in this order. */
#define RB_WK(n) ((VALUE)((n) << 3))
#define rb_cObject         RB_WK(1)
#define rb_cBasicObject    RB_WK(2)
#define rb_mKernel         RB_WK(3)
#define rb_cString         RB_WK(4)
#define rb_cArray          RB_WK(5)
#define rb_cHash           RB_WK(6)
#define rb_cInteger        RB_WK(7)
#define rb_cFloat          RB_WK(8)
#define rb_cSymbol         RB_WK(9)
#define rb_cProc           RB_WK(10)
#define rb_cModule         RB_WK(11)
#define rb_cClass          RB_WK(12)
#define rb_cNilClass       RB_WK(13)
#define rb_cTrueClass      RB_WK(14)
#define rb_cFalseClass     RB_WK(15)
#define rb_cIO             RB_WK(16)
#define rb_cTime           RB_WK(17)
#define rb_cEncoding       RB_WK(18)
#define rb_mComparable     RB_WK(19)
#define rb_mEnumerable     RB_WK(20)
#define rb_eException      RB_WK(21)
#define rb_eStandardError  RB_WK(22)
#define rb_eRuntimeError   RB_WK(23)
#define rb_eArgError       RB_WK(24)
#define rb_eTypeError      RB_WK(25)
#define rb_eRangeError     RB_WK(26)
#define rb_eIOError        RB_WK(27)
#define rb_eNoMemError     RB_WK(28)
#define rb_eNotImpError    RB_WK(29)
#define rb_eIndexError     RB_WK(30)
#define rb_eKeyError       RB_WK(31)
#define rb_eStopIteration  RB_WK(32)
#define rb_eEncodingError  RB_WK(33)
#define rb_eFrozenError    RB_WK(34)
#define rb_eZeroDivError   RB_WK(35)
#define rb_eNameError      RB_WK(36)
#define rb_eNoMethodError  RB_WK(37)
#define rb_eSecurityError  RB_WK(38)
#define rb_eSystemCallError RB_WK(39)
#define rb_eEncCompatError RB_WK(40)
#define rb_cRange          RB_WK(41)
#define rb_cRegexp         RB_WK(42)
#define rb_cStruct         RB_WK(43)
#define rb_cNumeric        RB_WK(44)
#define rb_cThread         RB_WK(45)
#define rb_stdout          RB_WK(46)
#define rb_stderr          RB_WK(47)
#define RB_WK_COUNT 47

RB_IMPORT(rb_ll2inum) VALUE rb_ll2inum(long long);
RB_IMPORT(rb_ull2inum) VALUE rb_ull2inum(unsigned long long);
RB_IMPORT(rb_num2ll) long long rb_num2ll(VALUE);
RB_IMPORT(rb_num2ull) unsigned long long rb_num2ull(VALUE);
RB_IMPORT(rb_num2dbl) double rb_num2dbl(VALUE);
RB_IMPORT(rb_float_new) VALUE rb_float_new(double);
RB_IMPORT(rb_dw_int64_pack) int rb_dw_int64_pack(VALUE, long long *);
int rb_integer_pack(VALUE, void *, size_t, size_t, size_t, int);
#define INTEGER_PACK_MSWORD_FIRST 0x01
#define INTEGER_PACK_LSWORD_FIRST 0x02
#define INTEGER_PACK_MSBYTE_FIRST 0x10
#define INTEGER_PACK_LSBYTE_FIRST 0x20
#define INTEGER_PACK_NATIVE_BYTE_ORDER 0x40
#define INTEGER_PACK_2COMP 0x80
#define INTEGER_PACK_LITTLE_ENDIAN (INTEGER_PACK_LSWORD_FIRST | INTEGER_PACK_LSBYTE_FIRST)
#define INTEGER_PACK_BIG_ENDIAN (INTEGER_PACK_MSWORD_FIRST | INTEGER_PACK_MSBYTE_FIRST)

static inline VALUE LONG2NUM(long i) { return FIXABLE(i) ? LONG2FIX(i) : rb_ll2inum(i); }
static inline VALUE ULONG2NUM(unsigned long i) { return POSFIXABLE(i) ? LONG2FIX((long)i) : rb_ull2inum(i); }
static inline VALUE LL2NUM(long long i) { return FIXABLE(i) ? LONG2FIX((long)i) : rb_ll2inum(i); }
static inline VALUE ULL2NUM(unsigned long long i) { return POSFIXABLE(i) ? LONG2FIX((long)i) : rb_ull2inum(i); }
#define INT2NUM(i) LONG2NUM((long)(i))
#define UINT2NUM(i) ULONG2NUM((unsigned long)(i))
#define SIZET2NUM(i) ULONG2NUM((unsigned long)(i))
#define OFFT2NUM(i) LL2NUM((long long)(i))
static inline long NUM2LONG(VALUE v) { return FIXNUM_P(v) ? FIX2LONG(v) : (long)rb_num2ll(v); }
static inline unsigned long NUM2ULONG(VALUE v) { return FIXNUM_P(v) ? (unsigned long)FIX2LONG(v) : (unsigned long)rb_num2ull(v); }
static inline long long NUM2LL(VALUE v) { return FIXNUM_P(v) ? (long long)FIX2LONG(v) : rb_num2ll(v); }
static inline unsigned long long NUM2ULL(VALUE v) { return FIXNUM_P(v) ? (unsigned long long)FIX2LONG(v) : rb_num2ull(v); }
#define NUM2INT(v) ((int)NUM2LONG(v))
#define NUM2UINT(v) ((unsigned int)NUM2ULONG(v))
#define NUM2SIZET(v) ((size_t)NUM2ULONG(v))
#define NUM2DBL(v) rb_num2dbl(v)
#define DBL2NUM(d) rb_float_new(d)
#define rb_int2inum(i) LONG2NUM(i)
#define rb_uint2inum(i) ULONG2NUM(i)

enum ruby_value_type {
    T_NONE = 0x00, T_OBJECT = 0x01, T_CLASS = 0x02, T_MODULE = 0x03, T_FLOAT = 0x04,
    T_STRING = 0x05, T_REGEXP = 0x06, T_ARRAY = 0x07, T_HASH = 0x08, T_STRUCT = 0x09,
    T_BIGNUM = 0x0a, T_FILE = 0x0b, T_DATA = 0x0c, T_MATCH = 0x0d, T_COMPLEX = 0x0e,
    T_RATIONAL = 0x0f, T_NIL = 0x11, T_TRUE = 0x12, T_FALSE = 0x13, T_SYMBOL = 0x14,
    T_FIXNUM = 0x15, T_UNDEF = 0x16,
};
RB_IMPORT(rb_dw_type) int rb_dw_type(VALUE);
static inline int rb_type(VALUE v) {
    if (FIXNUM_P(v)) return T_FIXNUM;
    switch (v) { case Qnil: return T_NIL; case Qtrue: return T_TRUE; case Qfalse: return T_FALSE; case Qundef: return T_UNDEF; }
    return rb_dw_type(v);
}
#define TYPE(v) rb_type((VALUE)(v))
#define RB_TYPE_P(v, t) (rb_type((VALUE)(v)) == (t))
#define SYMBOL_P(v) RB_TYPE_P(v, T_SYMBOL)
#define RB_FLOAT_TYPE_P(v) RB_TYPE_P(v, T_FLOAT)
#define RB_INTEGER_TYPE_P(v) (FIXNUM_P(v) || RB_TYPE_P(v, T_BIGNUM))
RB_IMPORT(rb_check_type) void rb_check_type(VALUE, int);
#define Check_Type(v, t) rb_check_type((VALUE)(v), (t))
RB_IMPORT(rb_obj_class) VALUE rb_obj_class(VALUE);
RB_IMPORT(rb_class_of) VALUE rb_class_of(VALUE);
#define CLASS_OF(v) rb_class_of(v)
RB_IMPORT(rb_obj_is_kind_of) VALUE rb_obj_is_kind_of(VALUE, VALUE);
RB_IMPORT(rb_obj_is_instance_of) VALUE rb_obj_is_instance_of(VALUE, VALUE);
RB_IMPORT(rb_respond_to) int rb_respond_to(VALUE, ID);
RB_IMPORT(rb_obj_frozen_p) VALUE rb_obj_frozen_p(VALUE);
#define OBJ_FROZEN(v) RTEST(rb_obj_frozen_p(v))
#define OBJ_FREEZE(v) rb_obj_freeze(v)
RB_IMPORT(rb_obj_freeze) VALUE rb_obj_freeze(VALUE);
RB_IMPORT(rb_check_frozen) void rb_check_frozen(VALUE);
RB_IMPORT(rb_equal) VALUE rb_equal(VALUE, VALUE);
RB_IMPORT(rb_eql) int rb_eql(VALUE, VALUE);
RB_IMPORT(rb_inspect) VALUE rb_inspect(VALUE);
RB_IMPORT(rb_obj_as_string) VALUE rb_obj_as_string(VALUE);
RB_IMPORT(rb_obj_id) VALUE rb_obj_id(VALUE);
RB_IMPORT(rb_obj_dup) VALUE rb_obj_dup(VALUE);
RB_IMPORT(rb_obj_alloc) VALUE rb_obj_alloc(VALUE);
RB_IMPORT(rb_class_new_instance) VALUE rb_class_new_instance(int, const VALUE *, VALUE);
RB_IMPORT(rb_class_new_instance_kw) VALUE rb_class_new_instance_kw(int, const VALUE *, VALUE, int);
RB_IMPORT(rb_obj_call_init) void rb_obj_call_init(VALUE, int, const VALUE *);
RB_IMPORT(rb_obj_call_init_kw) void rb_obj_call_init_kw(VALUE, int, const VALUE *, int);
RB_IMPORT(rb_obj_classname) const char *rb_obj_classname(VALUE);
RB_IMPORT(rb_class2name) const char *rb_class2name(VALUE);
RB_IMPORT(rb_class_new) VALUE rb_class_new(VALUE);
RB_IMPORT(rb_call_super) VALUE rb_call_super(int, const VALUE *);
RB_IMPORT(rb_get_kwargs) int rb_get_kwargs(VALUE, const ID *, int, int, VALUE *);
RB_IMPORT(rb_proc_arity) int rb_proc_arity(VALUE);
RB_IMPORT(rb_apply) VALUE rb_apply(VALUE, ID, VALUE);
RB_IMPORT(rb_str_split) VALUE rb_str_split(VALUE, const char *);
#define RB_PASS_KEYWORDS 1
#define RB_NO_KEYWORDS 0
#define RB_PASS_CALLED_KEYWORDS rb_keyword_given_p()
RB_IMPORT(rb_keyword_given_p) int rb_keyword_given_p(void);
RB_IMPORT(rb_class_name) VALUE rb_class_name(VALUE);

RB_IMPORT(rb_intern) ID rb_intern(const char *);
RB_IMPORT(rb_intern2) ID rb_intern2(const char *, long);
RB_IMPORT(rb_intern_str) ID rb_intern_str(VALUE);
RB_IMPORT(rb_id2name) const char *rb_id2name(ID);
RB_IMPORT(rb_sym2str) VALUE rb_sym2str(VALUE);
#define rb_id2str(id) rb_sym2str((VALUE)(id))
#define ID2SYM(id) ((VALUE)(id))
#define SYM2ID(sym) ((ID)(sym))
#define rb_sym2id(sym) ((ID)(sym))
#define rb_id2sym(id) ((VALUE)(id))
#define rb_to_id(v) rb_intern_str(rb_obj_as_string(v))
#define rb_intern_const rb_intern
#define CONST_ID(var, str) ((var) = rb_intern(str))

RB_IMPORT(rb_define_module) VALUE rb_define_module(const char *);
RB_IMPORT(rb_define_module_under) VALUE rb_define_module_under(VALUE, const char *);
RB_IMPORT(rb_define_class) VALUE rb_define_class(const char *, VALUE);
RB_IMPORT(rb_define_class_under) VALUE rb_define_class_under(VALUE, const char *, VALUE);
RB_IMPORT(rb_define_method) void rb_define_method(VALUE, const char *, VALUE (*)(ANYARGS), int);
RB_IMPORT(rb_define_private_method) void rb_define_private_method(VALUE, const char *, VALUE (*)(ANYARGS), int);
RB_IMPORT(rb_define_singleton_method) void rb_define_singleton_method(VALUE, const char *, VALUE (*)(ANYARGS), int);
RB_IMPORT(rb_define_module_function) void rb_define_module_function(VALUE, const char *, VALUE (*)(ANYARGS), int);
RB_IMPORT(rb_define_alloc_func) void rb_define_alloc_func(VALUE, VALUE (*)(VALUE));
RB_IMPORT(rb_undef_alloc_func) void rb_undef_alloc_func(VALUE);
RB_IMPORT(rb_define_const) void rb_define_const(VALUE, const char *, VALUE);
RB_IMPORT(rb_define_alias) void rb_define_alias(VALUE, const char *, const char *);
RB_IMPORT(rb_define_attr) void rb_define_attr(VALUE, const char *, int, int);
RB_IMPORT(rb_undef_method) void rb_undef_method(VALUE, const char *);
RB_IMPORT(rb_include_module) void rb_include_module(VALUE, VALUE);
RB_IMPORT(rb_extend_object) void rb_extend_object(VALUE, VALUE);
RB_IMPORT(rb_const_get) VALUE rb_const_get(VALUE, ID);
RB_IMPORT(rb_const_get_at) VALUE rb_const_get_at(VALUE, ID);
RB_IMPORT(rb_const_defined) int rb_const_defined(VALUE, ID);
RB_IMPORT(rb_const_set) void rb_const_set(VALUE, ID, VALUE);
RB_IMPORT(rb_path2class) VALUE rb_path2class(const char *);
RB_IMPORT(rb_define_protected_method) void rb_define_protected_method(VALUE, const char *, VALUE (*)(ANYARGS), int);
RB_IMPORT(rb_ivar_defined) VALUE rb_ivar_defined(VALUE, ID);
RB_IMPORT(rb_ivar_get) VALUE rb_ivar_get(VALUE, ID);
RB_IMPORT(rb_ivar_set) VALUE rb_ivar_set(VALUE, ID, VALUE);
RB_IMPORT(rb_iv_get) VALUE rb_iv_get(VALUE, const char *);
RB_IMPORT(rb_iv_set) VALUE rb_iv_set(VALUE, const char *, VALUE);
RB_IMPORT(rb_cv_get) VALUE rb_cv_get(VALUE, const char *);
RB_IMPORT(rb_cv_set) void rb_cv_set(VALUE, const char *, VALUE);
RB_IMPORT(rb_gv_get) VALUE rb_gv_get(const char *);
RB_IMPORT(rb_ext_ractor_safe) void rb_ext_ractor_safe(int);

RB_IMPORT(rb_funcallv) VALUE rb_funcallv(VALUE, ID, int, const VALUE *);
RB_IMPORT(rb_funcallv_public) VALUE rb_funcallv_public(VALUE, ID, int, const VALUE *);
RB_IMPORT(rb_funcall_with_block) VALUE rb_funcall_with_block(VALUE, ID, int, const VALUE *, VALUE);
static inline VALUE rb_funcall(VALUE recv, ID mid, int n, ...) {
    VALUE argv[16];
    va_list ap;
    va_start(ap, n);
    for (int i = 0; i < n; i++) argv[i] = va_arg(ap, VALUE);
    va_end(ap);
    return rb_funcallv(recv, mid, n, argv);
}
#define rb_funcall2 rb_funcallv
#define rb_funcall3 rb_funcallv_public
RB_IMPORT(rb_block_given_p) int rb_block_given_p(void);
RB_IMPORT(rb_yield) VALUE rb_yield(VALUE);
RB_IMPORT(rb_yield_values2) VALUE rb_yield_values2(int, const VALUE *);
RB_IMPORT(rb_yield_splat) VALUE rb_yield_splat(VALUE);
RB_IMPORT(rb_block_proc) VALUE rb_block_proc(void);
RB_IMPORT(rb_proc_call) VALUE rb_proc_call(VALUE, VALUE);
RB_IMPORT(rb_proc_new) VALUE rb_proc_new(VALUE (*)(ANYARGS), VALUE);
RB_IMPORT(rb_block_call) VALUE rb_block_call(VALUE, ID, int, const VALUE *, VALUE (*)(ANYARGS), VALUE);
RB_IMPORT(rb_need_block) void rb_need_block(void);
int rb_scan_args(int argc, const VALUE *argv, const char *fmt, ...);

RB_IMPORT(rb_exc_raise) RB_NORETURN void rb_exc_raise(VALUE);
RB_IMPORT(rb_exc_new_str) VALUE rb_exc_new_str(VALUE, VALUE);
RB_IMPORT(rb_exc_new) VALUE rb_exc_new(VALUE, const char *, long);
RB_IMPORT(rb_dw_warn_str) void rb_dw_warn_str(VALUE);
RB_IMPORT(rb_dw_bug_str) RB_NORETURN void rb_dw_bug_str(VALUE);
#define PRIsVALUE "li\v"
RB_NORETURN void rb_raise(VALUE, const char *, ...);
void rb_warn(const char *, ...);
#define rb_warning rb_warn
#define rb_category_warning(category, ...) rb_warn(__VA_ARGS__)
#define rb_category_warn(category, ...) rb_warn(__VA_ARGS__)
RB_NORETURN void rb_bug(const char *, ...);
RB_NORETURN void rb_fatal(const char *, ...);
VALUE rb_sprintf(const char *, ...);
VALUE rb_vsprintf(const char *, va_list);
VALUE rb_str_catf(VALUE, const char *, ...);
VALUE rb_exc_new_str_f(VALUE, const char *, ...);
#define rb_enc_sprintf(enc, ...) rb_sprintf(__VA_ARGS__)
RB_IMPORT(rb_protect) VALUE rb_protect(VALUE (*)(VALUE), VALUE, int *);
RB_IMPORT(rb_ensure) VALUE rb_ensure(VALUE (*)(VALUE), VALUE, VALUE (*)(VALUE), VALUE);
RB_IMPORT(rb_rescue) VALUE rb_rescue(VALUE (*)(VALUE), VALUE, VALUE (*)(VALUE, VALUE), VALUE);
RB_IMPORT(rb_jump_tag) RB_NORETURN void rb_jump_tag(int);
RB_IMPORT(rb_errinfo) VALUE rb_errinfo(void);
RB_IMPORT(rb_set_errinfo) void rb_set_errinfo(VALUE);
RB_IMPORT(rb_sys_fail) RB_NORETURN void rb_sys_fail(const char *);
RB_IMPORT(rb_iter_break) RB_NORETURN void rb_iter_break(void);
RB_IMPORT(rb_exc_new_cstr) VALUE rb_exc_new_cstr(VALUE, const char *);
#define rb_exc_new2 rb_exc_new_cstr
#define rb_exc_new3 rb_exc_new_str
enum { TAG_NONE = 0, TAG_RETURN = 1, TAG_BREAK = 2, TAG_NEXT = 3, TAG_RETRY = 4, TAG_REDO = 5, TAG_RAISE = 6, TAG_THROW = 7, TAG_FATAL = 8 };

RB_IMPORT(rb_str_new) VALUE rb_str_new(const char *, long);
RB_IMPORT(rb_str_new_cstr) VALUE rb_str_new_cstr(const char *);
RB_IMPORT(rb_utf8_str_new) VALUE rb_utf8_str_new(const char *, long);
RB_IMPORT(rb_utf8_str_new_cstr) VALUE rb_utf8_str_new_cstr(const char *);
RB_IMPORT(rb_str_new_frozen) VALUE rb_str_new_frozen(VALUE);
RB_IMPORT(rb_str_dup) VALUE rb_str_dup(VALUE);
RB_IMPORT(rb_str_buf_new) VALUE rb_str_buf_new(long);
RB_IMPORT(rb_str_cat) VALUE rb_str_cat(VALUE, const char *, long);
RB_IMPORT(rb_str_cat_cstr) VALUE rb_str_cat_cstr(VALUE, const char *);
RB_IMPORT(rb_str_append) VALUE rb_str_append(VALUE, VALUE);
RB_IMPORT(rb_str_concat) VALUE rb_str_concat(VALUE, VALUE);
RB_IMPORT(rb_str_plus) VALUE rb_str_plus(VALUE, VALUE);
RB_IMPORT(rb_str_length) VALUE rb_str_length(VALUE);
RB_IMPORT(rb_str_freeze) VALUE rb_str_freeze(VALUE);
RB_IMPORT(rb_str_to_str) VALUE rb_str_to_str(VALUE);
RB_IMPORT(rb_str_intern) VALUE rb_str_intern(VALUE);
RB_IMPORT(rb_str_equal) VALUE rb_str_equal(VALUE, VALUE);
RB_IMPORT(rb_str_set_len) void rb_str_set_len(VALUE, long);
RB_IMPORT(rb_str_resize) VALUE rb_str_resize(VALUE, long);
RB_IMPORT(rb_str_modify) void rb_str_modify(VALUE);
RB_IMPORT(rb_dw_string_value) VALUE rb_dw_string_value(VALUE);
RB_IMPORT(rb_dw_str_ptr) char *rb_dw_str_ptr(VALUE);
RB_IMPORT(rb_dw_str_len) long rb_dw_str_len(VALUE);
RB_IMPORT(rb_check_string_type) VALUE rb_check_string_type(VALUE);
RB_IMPORT(rb_obj_encoding) VALUE rb_obj_encoding(VALUE);
static inline VALUE rb_string_value(volatile VALUE *p) { *p = rb_dw_string_value(*p); return *p; }
static inline char *rb_string_value_ptr(volatile VALUE *p) { return rb_dw_str_ptr(rb_string_value(p)); }
static inline char *rb_string_value_cstr(volatile VALUE *p) {
    VALUE s = rb_string_value(p);
    char *ptr = rb_dw_str_ptr(s);
    if (strlen(ptr) != (size_t)rb_dw_str_len(s)) rb_raise(rb_eArgError, "string contains null byte");
    return ptr;
}
#define StringValue(v) rb_string_value(&(v))
#define StringValuePtr(v) rb_string_value_ptr(&(v))
#define StringValueCStr(v) rb_string_value_cstr(&(v))
#define RSTRING_PTR(v) rb_dw_str_ptr(v)
#define RSTRING_LEN(v) rb_dw_str_len(v)
#define RSTRING_LENINT(v) ((int)rb_dw_str_len(v))
#define RSTRING_END(v) (RSTRING_PTR(v) + RSTRING_LEN(v))
#define RSTRING_GETMEM(v, p, l) ((p) = RSTRING_PTR(v), (l) = RSTRING_LEN(v))
#define rb_str_new2 rb_str_new_cstr
#define rb_str_new3 rb_str_new_frozen
#define rb_str_new4 rb_str_new_frozen
#define rb_str_cat2 rb_str_cat_cstr
#define rb_str_buf_cat rb_str_cat
#define rb_str_buf_cat2 rb_str_cat_cstr
#define rb_str_buf_append rb_str_append
#define rb_external_str_new rb_str_new
#define rb_external_str_new_cstr rb_str_new_cstr
#define rb_locale_str_new rb_str_new
#define rb_usascii_str_new rb_str_new
#define rb_usascii_str_new_cstr rb_str_new_cstr
#define rb_str_new_static rb_str_new
#define rb_str_new_literal(s) rb_str_new((s), sizeof(s) - 1)
#define rb_utf8_str_new_literal(s) rb_utf8_str_new((s), sizeof(s) - 1)
#define rb_str_new_lit rb_str_new_literal
#define rb_utf8_str_new_lit rb_utf8_str_new_literal
#define rb_utf8_str_new_static rb_utf8_str_new
#define rb_usascii_str_new_static rb_str_new
#define rb_sym2str_cstr(s) rb_dw_str_ptr(rb_sym2str(s))

typedef struct rb_encoding rb_encoding;
RB_IMPORT(rb_utf8_encoding) rb_encoding *rb_utf8_encoding(void);
RB_IMPORT(rb_ascii8bit_encoding) rb_encoding *rb_ascii8bit_encoding(void);
RB_IMPORT(rb_usascii_encoding) rb_encoding *rb_usascii_encoding(void);
RB_IMPORT(rb_default_internal_encoding) rb_encoding *rb_default_internal_encoding(void);
RB_IMPORT(rb_default_external_encoding) rb_encoding *rb_default_external_encoding(void);
RB_IMPORT(rb_locale_encoding) rb_encoding *rb_locale_encoding(void);
RB_IMPORT(rb_enc_from_index) rb_encoding *rb_enc_from_index(int);
RB_IMPORT(rb_enc_to_index) int rb_enc_to_index(rb_encoding *);
RB_IMPORT(rb_enc_find_index) int rb_enc_find_index(const char *);
RB_IMPORT(rb_enc_find) rb_encoding *rb_enc_find(const char *);
RB_IMPORT(rb_enc_get_index) int rb_enc_get_index(VALUE);
RB_IMPORT(rb_enc_get) rb_encoding *rb_enc_get(VALUE);
RB_IMPORT(rb_enc_associate) VALUE rb_enc_associate(VALUE, rb_encoding *);
RB_IMPORT(rb_enc_associate_index) VALUE rb_enc_associate_index(VALUE, int);
RB_IMPORT(rb_enc_from_encoding) VALUE rb_enc_from_encoding(rb_encoding *);
RB_IMPORT(rb_to_encoding) rb_encoding *rb_to_encoding(VALUE);
RB_IMPORT(rb_to_encoding_index) int rb_to_encoding_index(VALUE);
RB_IMPORT(rb_enc_name) const char *rb_enc_name(rb_encoding *);
RB_IMPORT(rb_enc_str_new) VALUE rb_enc_str_new(const char *, long, rb_encoding *);
RB_IMPORT(rb_enc_str_new_cstr) VALUE rb_enc_str_new_cstr(const char *, rb_encoding *);
RB_IMPORT(rb_enc_interned_str) VALUE rb_enc_interned_str(const char *, long, rb_encoding *);
RB_IMPORT(rb_enc_interned_str_cstr) VALUE rb_enc_interned_str_cstr(const char *, rb_encoding *);
RB_IMPORT(rb_enc_str_buf_cat) VALUE rb_enc_str_buf_cat(VALUE, const char *, long, rb_encoding *);
RB_IMPORT(rb_str_export_to_enc) VALUE rb_str_export_to_enc(VALUE, rb_encoding *);
RB_IMPORT(rb_str_conv_enc) VALUE rb_str_conv_enc(VALUE, rb_encoding *, rb_encoding *);
RB_IMPORT(rb_str_encode) VALUE rb_str_encode(VALUE, VALUE, int, VALUE);
RB_IMPORT(rb_enc_str_asciionly_p) int rb_enc_str_asciionly_p(VALUE);
RB_IMPORT(rb_utf8_encindex) int rb_utf8_encindex(void);
RB_IMPORT(rb_ascii8bit_encindex) int rb_ascii8bit_encindex(void);
RB_IMPORT(rb_usascii_encindex) int rb_usascii_encindex(void);
#define rb_external_str_new_with_enc(p, n, e) rb_enc_str_new((p), (n), (e))
#define rb_enc_str_new_static rb_enc_str_new
#define rb_enc_str_new_literal(s, e) rb_enc_str_new((s), sizeof(s) - 1, (e))
#define rb_enc_copy(dst, src) rb_enc_associate((dst), rb_enc_get(src))
#define ENCODING_GET(v) rb_enc_get_index(v)
#define ENC_CODERANGE_CLEAR(v) ((void)0)
#define ENC_CODERANGE_7BIT 1
#define rb_enc_asciicompat(enc) 1
#define rb_enc_mbminlen(enc) 1
#define rb_enc_dummy_p(enc) 0

RB_IMPORT(rb_ary_new) VALUE rb_ary_new(void);
RB_IMPORT(rb_ary_new_capa) VALUE rb_ary_new_capa(long);
RB_IMPORT(rb_ary_new_from_values) VALUE rb_ary_new_from_values(long, const VALUE *);
static inline VALUE rb_ary_new_from_args(long n, ...) {
    VALUE argv[16];
    va_list ap;
    va_start(ap, n);
    for (long i = 0; i < n; i++) argv[i] = va_arg(ap, VALUE);
    va_end(ap);
    return rb_ary_new_from_values(n, argv);
}
#define rb_ary_new2 rb_ary_new_capa
#define rb_ary_new3 rb_ary_new_from_args
#define rb_ary_new4 rb_ary_new_from_values
RB_IMPORT(rb_ary_push) VALUE rb_ary_push(VALUE, VALUE);
RB_IMPORT(rb_ary_pop) VALUE rb_ary_pop(VALUE);
RB_IMPORT(rb_ary_shift) VALUE rb_ary_shift(VALUE);
RB_IMPORT(rb_ary_unshift) VALUE rb_ary_unshift(VALUE, VALUE);
RB_IMPORT(rb_ary_entry) VALUE rb_ary_entry(VALUE, long);
RB_IMPORT(rb_ary_store) void rb_ary_store(VALUE, long, VALUE);
RB_IMPORT(rb_ary_dup) VALUE rb_ary_dup(VALUE);
RB_IMPORT(rb_ary_clear) VALUE rb_ary_clear(VALUE);
RB_IMPORT(rb_ary_join) VALUE rb_ary_join(VALUE, VALUE);
RB_IMPORT(rb_ary_concat) VALUE rb_ary_concat(VALUE, VALUE);
RB_IMPORT(rb_ary_includes) VALUE rb_ary_includes(VALUE, VALUE);
RB_IMPORT(rb_ary_delete) VALUE rb_ary_delete(VALUE, VALUE);
RB_IMPORT(rb_ary_reverse) VALUE rb_ary_reverse(VALUE);
RB_IMPORT(rb_ary_to_ary) VALUE rb_ary_to_ary(VALUE);
RB_IMPORT(rb_check_array_type) VALUE rb_check_array_type(VALUE);
RB_IMPORT(rb_dw_array_len) long rb_dw_array_len(VALUE);
#define RARRAY_LEN(v) rb_dw_array_len(v)
#define RARRAY_AREF(v, i) rb_ary_entry((v), (i))
#define rb_ary_aref1(v, i) rb_ary_entry((v), NUM2LONG(i))
#define Check_Array_Type(v) rb_check_array_type(v)
#define rb_Array(v) rb_check_array_type(v)

RB_IMPORT(rb_hash_new) VALUE rb_hash_new(void);
RB_IMPORT(rb_hash_aref) VALUE rb_hash_aref(VALUE, VALUE);
RB_IMPORT(rb_hash_aset) VALUE rb_hash_aset(VALUE, VALUE, VALUE);
RB_IMPORT(rb_hash_lookup) VALUE rb_hash_lookup(VALUE, VALUE);
RB_IMPORT(rb_hash_lookup2) VALUE rb_hash_lookup2(VALUE, VALUE, VALUE);
RB_IMPORT(rb_hash_fetch) VALUE rb_hash_fetch(VALUE, VALUE);
RB_IMPORT(rb_hash_delete) VALUE rb_hash_delete(VALUE, VALUE);
RB_IMPORT(rb_hash_size) VALUE rb_hash_size(VALUE);
RB_IMPORT(rb_hash_keys) VALUE rb_hash_keys(VALUE);
RB_IMPORT(rb_hash_dup) VALUE rb_hash_dup(VALUE);
RB_IMPORT(rb_hash_clear) VALUE rb_hash_clear(VALUE);
RB_IMPORT(rb_check_hash_type) VALUE rb_check_hash_type(VALUE);
RB_IMPORT(rb_hash_foreach) void rb_hash_foreach(VALUE, int (*)(VALUE, VALUE, VALUE), VALUE);
RB_IMPORT(rb_dw_hash_size_num) long rb_dw_hash_size_num(VALUE);
#define RHASH_SIZE(v) rb_dw_hash_size_num(v)
#define RHASH_EMPTY_P(v) (RHASH_SIZE(v) == 0)

typedef void (*RUBY_DATA_FUNC)(void *);
struct rb_data_type_struct {
    const char *wrap_struct_name;
    struct {
        RUBY_DATA_FUNC dmark;
        RUBY_DATA_FUNC dfree;
        size_t (*dsize)(const void *);
        RUBY_DATA_FUNC dcompact;
        void *reserved[1];
    } function;
    const struct rb_data_type_struct *parent;
    void *data;
    VALUE flags;
};
typedef struct rb_data_type_struct rb_data_type_t;
#define RUBY_TYPED_FREE_IMMEDIATELY 1
#define RUBY_TYPED_FROZEN_SHAREABLE 0
#define RUBY_TYPED_WB_PROTECTED 0
#define RUBY_TYPED_DEFAULT_FREE ((RUBY_DATA_FUNC)-1)
#define RUBY_DEFAULT_FREE ((RUBY_DATA_FUNC)-1)
#define RUBY_NEVER_FREE ((RUBY_DATA_FUNC)0)
RB_IMPORT(rb_data_typed_object_wrap) VALUE rb_data_typed_object_wrap(VALUE, void *, const rb_data_type_t *);
RB_IMPORT(rb_data_object_wrap) VALUE rb_data_object_wrap(VALUE, void *, RUBY_DATA_FUNC, RUBY_DATA_FUNC);
RB_IMPORT(rb_check_typeddata) void *rb_check_typeddata(VALUE, const rb_data_type_t *);
RB_IMPORT(rb_typeddata_is_kind_of) int rb_typeddata_is_kind_of(VALUE, const rb_data_type_t *);
RB_IMPORT(rb_dw_data_ptr) void *rb_dw_data_ptr(VALUE);
RB_IMPORT(rb_dw_data_set_ptr) void rb_dw_data_set_ptr(VALUE, void *);
RB_IMPORT(rb_dw_data_ptr_ref) void **rb_dw_data_ptr_ref(VALUE);
RB_IMPORT(rb_dw_data_type_p) int rb_dw_data_type_p(VALUE, const rb_data_type_t *);
#define TypedData_Wrap_Struct(klass, type, sval) rb_data_typed_object_wrap((klass), (sval), (type))
#define TypedData_Make_Struct(klass, T, type, sval) \
    rb_data_typed_object_wrap((klass), ((sval) = (T *)calloc(1, sizeof(T))), (type))
#define TypedData_Get_Struct(obj, T, type, sval) ((sval) = (T *)rb_check_typeddata((obj), (type)))
#define Data_Wrap_Struct(klass, mark, free, sval) rb_data_object_wrap((klass), (sval), (RUBY_DATA_FUNC)(mark), (RUBY_DATA_FUNC)(free))
#define Data_Get_Struct(obj, T, sval) ((sval) = (T *)rb_dw_data_ptr(obj))
#define RTYPEDDATA_DATA(v) rb_dw_data_ptr(v)
#define DATA_PTR(v) (*rb_dw_data_ptr_ref(v))
#define RTYPEDDATA_P(v) (rb_dw_type(v) == T_DATA)
#define rb_typeddata_inherited_p(a, b) ((a) == (b))

RB_IMPORT(rb_gc_mark) void rb_gc_mark(VALUE);
#define rb_gc_mark_movable rb_gc_mark
RB_IMPORT(rb_gc_register_address) void rb_gc_register_address(VALUE *);
#define rb_global_variable rb_gc_register_address
RB_IMPORT(rb_gc_unregister_address) void rb_gc_unregister_address(VALUE *);
RB_IMPORT(rb_gc_register_mark_object) void rb_gc_register_mark_object(VALUE);
RB_IMPORT(rb_gc) void rb_gc(void);
#define RB_GC_GUARD(v) ((void)(v))
#define rb_gc_writebarrier(a, b) ((void)0)
#define RB_OBJ_WRITE(a, slot, b) (*(slot) = (b))
#define RB_OBJ_WRITTEN(a, oldv, b) ((void)0)
#define rb_gc_location(v) (v)
#define rb_gc_mark_locations(a, b) ((void)0)

#define xmalloc malloc
#define xcalloc calloc
#define xrealloc realloc
#define xfree free
#define ruby_xmalloc malloc
#define ruby_xcalloc calloc
#define ruby_xrealloc realloc
#define ruby_xfree free
#define ALLOC(T) ((T *)malloc(sizeof(T)))
#define ALLOC_N(T, n) ((T *)malloc(sizeof(T) * (n)))
#define ZALLOC(T) ((T *)calloc(1, sizeof(T)))
#define ZALLOC_N(T, n) ((T *)calloc((n), sizeof(T)))
#define REALLOC_N(p, T, n) ((p) = (T *)realloc((p), sizeof(T) * (n)))
#define ALLOCA_N(T, n) ((T *)alloca(sizeof(T) * (n)))
#define ALLOCV_N(T, v, n) ((T *)((v) = (VALUE)malloc(sizeof(T) * (n) + 1)))
#define ALLOCV(v, n) ALLOCV_N(char, v, n)
#define ALLOCV_END(v) free((void *)(v))
#define MEMCPY(d, s, T, n) memcpy((d), (s), sizeof(T) * (n))
#define MEMZERO(p, T, n) memset((p), 0, sizeof(T) * (n))
#define MEMMOVE(d, s, T, n) memmove((d), (s), sizeof(T) * (n))

RB_IMPORT(rb_io_write) VALUE rb_io_write(VALUE, VALUE);
RB_IMPORT(rb_require) VALUE rb_require(const char *);
RB_IMPORT(rb_eval_string) VALUE rb_eval_string(const char *);
RB_IMPORT(rb_thread_schedule) void rb_thread_schedule(void);
RB_IMPORT(rb_range_new) VALUE rb_range_new(VALUE, VALUE, int);
RB_IMPORT(rb_range_beg_len) VALUE rb_range_beg_len(VALUE, long *, long *, long, int);
RB_IMPORT(rb_time_new) VALUE rb_time_new(long, long);
RB_IMPORT(rb_to_int) VALUE rb_to_int(VALUE);
RB_IMPORT(rb_Integer) VALUE rb_Integer(VALUE);
RB_IMPORT(rb_Float) VALUE rb_Float(VALUE);
RB_IMPORT(rb_String) VALUE rb_String(VALUE);
RB_IMPORT(rb_to_float) VALUE rb_to_float(VALUE);
RB_IMPORT(rb_num_coerce_bin) VALUE rb_num_coerce_bin(VALUE, VALUE, ID);
static inline void *rb_thread_call_without_gvl(void *(*func)(void *), void *data1, void (*ubf)(void *), void *data2) {
    (void)ubf; (void)data2;
    return func(data1);
}
#define rb_thread_call_without_gvl2 rb_thread_call_without_gvl
#define rb_nogvl(func, data1, ubf, data2, flags) rb_thread_call_without_gvl((func), (data1), (ubf), (data2))
#define RUBY_UBF_IO ((void (*)(void *))0)
#define RUBY_UBF_PROCESS ((void (*)(void *))0)
#define RB_NOGVL_UBF_ASYNC_SAFE 0
#define RB_NOGVL_INTR_FAIL 0

typedef int rb_pid_t;
pid_t getpid(void);
#define rb_check_safe_obj(v) ((void)(v))
#define RUBY_API_VERSION_MAJOR 4
#define RUBY_API_VERSION_MINOR 0
#define RUBY_VERSION "4.0.0"
#define RSTRING_NOEMBED 0
#define RUBY_FUNC_EXPORTED __attribute__((visibility("default")))
#define RUBY_FUNC_NONNULL(n, x) x
#define RBIMPL_ATTR_NORETURN() __attribute__((noreturn))
#define NORETURN(x) __attribute__((noreturn)) x
#define PRINTF_ARGS(decl, a, b) decl
#define UNREACHABLE __builtin_unreachable()
#define UNREACHABLE_RETURN(v) __builtin_unreachable()
#define RUBY_ASSERT(x) ((void)0)
#define RBIMPL_UNUSED(x) ((void)(x))
#define rb_ext_ractor_safe_dw(x) ((void)0)
#define HAVE_RB_EXT_RACTOR_SAFE 1
#define RB_UNLIKELY(x) __builtin_expect(!!(x), 0)
#define RB_LIKELY(x) __builtin_expect(!!(x), 1)

#endif
