// Copyright 2015 Nathan Sizemore <nathanrsizemore@gmail.com>
//
// This Source Code Form is subject to the terms of the
// Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at
// http://mozilla.org/MPL/2.0/.

#include <arpa/inet.h>

extern int shim_inet_pton(int af, const char *src, void *dst)
{
    return inet_pton(af, src, dst);
}

extern unsigned short shim_htons(unsigned short hostshort)
{
    return htons(hostshort);
}

extern unsigned int shim_htonl(unsigned int addr)
{
    return htonl(addr);
}

extern unsigned long int shim_inaddr_any()
{
    return INADDR_ANY;
}
