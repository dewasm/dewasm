/* <net/if.h> for the wasm32-wasip1 libpcap build: wasi-libc ships no net/
   headers, and pcap.c and nametoaddr.c include this one unconditionally.
   pcap_lookupnet compiles a struct ifreq with the two fields it touches
   (its SIOCGIFADDR ioctl can only fail at runtime, and no filter-compiler
   entry point reaches it); everything else libpcap would use from this
   header sits behind #ifdefs of macros wasi-libc does not define.
   See ../netdb.h for the companion resolver stand-in. */
#ifndef DEWASM_PCAP_NET_IF_H
#define DEWASM_PCAP_NET_IF_H

#include <sys/socket.h>

#define IFNAMSIZ 16

struct ifreq {
	char ifr_name[IFNAMSIZ];
	struct sockaddr ifr_addr;
};

#endif
