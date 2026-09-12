/* <netdb.h> for the wasm32-wasip1 libpcap build.
   wasi-libc ships no netdb.h (WASI preview1 has no name resolution), but
   libpcap's filter compiler includes it (pcap/socket.h, nametoaddr.c,
   gencode.c) and links five of its functions.
   This header declares only what those TUs use; stubs.c in this directory
   defines each function as a resolver-less failure, so a filter naming a
   host or service fails at pcap_compile time with "unknown host", not at
   build time on a missing header or symbol.  net/if.h beside it covers
   the other header those TUs include and wasi-libc lacks. */
#ifndef DEWASM_PCAP_NETDB_H
#define DEWASM_PCAP_NETDB_H

#include <stdint.h>
#include <sys/socket.h>

struct addrinfo {
	int ai_flags;
	int ai_family;
	int ai_socktype;
	int ai_protocol;
	socklen_t ai_addrlen;
	struct sockaddr *ai_addr;
	char *ai_canonname;
	struct addrinfo *ai_next;
};

struct hostent {
	char *h_name;
	char **h_aliases;
	int h_addrtype;
	int h_length;
	char **h_addr_list;
};
#define h_addr h_addr_list[0]

struct netent {
	char *n_name;
	char **n_aliases;
	int n_addrtype;
	uint32_t n_net;
};

struct protoent {
	char *p_name;
	char **p_aliases;
	int p_proto;
};

/* The values musl uses; gencode.c switches on EAI_NONAME/EAI_SERVICE. */
#define EAI_BADFLAGS -1
#define EAI_NONAME -2
#define EAI_AGAIN -3
#define EAI_FAIL -4
#define EAI_FAMILY -6
#define EAI_SOCKTYPE -7
#define EAI_SERVICE -8
#define EAI_MEMORY -10
#define EAI_OVERFLOW -12

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **res);
void freeaddrinfo(struct addrinfo *res);
struct hostent *gethostbyname(const char *name);
struct netent *getnetbyname(const char *name);
struct protoent *getprotobyname(const char *name);

#endif
