/* The definitions this directory's headers promise, plus socket(), which
   wasi-libc declares but does not define (WASI preview1 cannot create
   sockets).  Every function fails the way a sandboxed module with no
   resolver and no network does, so a filter naming a host fails at
   pcap_compile time and pcap_lookupnet returns an error.
   The e2e filter ("tcp port 80") is numeric and keyword-only, so none of
   these run in the tests; they exist so the link resolves. */
#include <errno.h>
#include <netdb.h>
#include <stddef.h>
#include <sys/socket.h>

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **res) {
	(void)node;
	(void)service;
	(void)hints;
	(void)res;
	return EAI_NONAME;
}

void freeaddrinfo(struct addrinfo *res) { (void)res; }

struct hostent *gethostbyname(const char *name) {
	(void)name;
	return NULL;
}

struct netent *getnetbyname(const char *name) {
	(void)name;
	return NULL;
}

struct protoent *getprotobyname(const char *name) {
	(void)name;
	return NULL;
}

int socket(int domain, int type, int protocol) {
	(void)domain;
	(void)type;
	(void)protocol;
	errno = ENOTSUP;
	return -1;
}
