/* A small `st_table`: the keyed table CRuby exposes to extensions, here a guest-local hash map.
 * It covers numeric keys (`st_init_numtable`) and C string keys (`st_init_strtable`).
 */
#ifndef DEWASM_RUBY_ST_H
#define DEWASM_RUBY_ST_H
#include <stddef.h>
#include <string.h>
#include <strings.h>

typedef unsigned long st_data_t;
typedef st_data_t st_index_t;
struct st_table;
typedef struct st_table st_table;
typedef int st_compare_func(st_data_t, st_data_t);
typedef st_index_t st_hash_func(st_data_t);
struct st_hash_type {
    st_compare_func *compare;
    st_hash_func *hash;
};
enum st_retval { ST_CONTINUE, ST_STOP, ST_DELETE, ST_CHECK, ST_REPLACE };

st_table *st_init_numtable(void);
st_table *st_init_numtable_with_size(st_index_t);
st_table *st_init_strtable(void);
st_table *st_init_strtable_with_size(st_index_t);
st_table *st_init_table(const struct st_hash_type *);
st_table *st_init_table_with_size(const struct st_hash_type *, st_index_t);
void st_free_table(st_table *);
int st_insert(st_table *, st_data_t, st_data_t);
int st_lookup(st_table *, st_data_t, st_data_t *);
int st_delete(st_table *, st_data_t *, st_data_t *);
int st_foreach(st_table *, int (*)(st_data_t, st_data_t, st_data_t), st_data_t);
st_index_t st_table_size(const st_table *);
size_t st_memsize(const st_table *);
#define st_strcasecmp strcasecmp
#define st_strncasecmp strncasecmp

#endif
