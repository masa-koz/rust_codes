#define __Windows__   // XXX
#define _GNU_SOURCE 1 // needed for struct in6_pktinfo
#if !defined(__Windows__)
#include <netinet/in.h>
#include <sys/socket.h>
#include <netinet/ip.h>
#include <string.h>
// constants to be exported in rust
int udp_sas_IP_PKTINFO = IP_PKTINFO;
int udp_sas_IPV6_RECVPKTINFO = IPV6_RECVPKTINFO;
#else
#include <stdio.h>
#include <Winsock2.h>
#include <Ws2tcpip.h>
#include <Mswsock.h>
// constants to be exported in rust
int udp_sas_IP_PKTINFO = IP_PKTINFO;
int udp_sas_IPV6_RECVPKTINFO = IPV6_PKTINFO;
#endif

#if !defined(__Windows__)
ssize_t recvmsg_for_quic(
    int sock,
#else
__int64 recvmsg_for_quic(
    int sock,
#endif
    void *buf, size_t buf_len, int flags,
    struct sockaddr *src, socklen_t src_len,
    struct sockaddr *dst, socklen_t dst_len)
{
#if !defined(__Windows__)
    struct iovec iov = {
        .iov_base = buf,
        .iov_len = buf_len};
    char control[256];
    struct msghdr msg = {
        .msg_name = src,
        .msg_namelen = src_len,
        .msg_iov = &iov,
        .msg_iovlen = 1,
        .msg_control = control,
        .msg_controllen = sizeof(control),
        .msg_flags = 0,
    };
#else
    WSABUF iov;
    iov.buf = buf;
    iov.len = buf_len;
    char ControlBuffer[WSA_CMSG_SPACE(sizeof(IN6_PKTINFO)) + // IP_PKTINFO
                       WSA_CMSG_SPACE(sizeof(DWORD)) +       // UDP_COALESCED_INFO
                       WSA_CMSG_SPACE(sizeof(INT))           // IP_ECN
    ];
    WSAMSG msg;
    GUID WSARecvMsg_GUID = WSAID_WSARECVMSG;
    LPFN_WSARECVMSG WSARecvMsg = NULL;

    DWORD ncounter = 0;
    int nResult;
    flags = 0;
#endif
    memset(src, 0, src_len);
    memset(dst, 0, dst_len);

#if !defined(__Windows__)
    ssize_t nb = recvmsg(sock, &msg, flags);
    if (nb >= 0)
#else
    nResult = WSAIoctl(sock, SIO_GET_EXTENSION_FUNCTION_POINTER,
                       &WSARecvMsg_GUID, sizeof WSARecvMsg_GUID,
                       &WSARecvMsg, sizeof WSARecvMsg,
                       &ncounter, NULL, NULL);
    if (nResult == 0)
    {
        msg.name = src;
        msg.namelen = src_len;
        msg.lpBuffers = &iov;
        msg.dwBufferCount = 1;
        msg.Control.buf = ControlBuffer;
        msg.Control.len = sizeof(ControlBuffer);
        msg.dwFlags = 0;
        nResult = WSARecvMsg(sock, &msg, &ncounter, NULL, NULL);
    }
    else
    {
        return nResult;
    }
    if (nResult == 0)
#endif
    {
        // parse the ancillary data
#if !defined(__Windows__)
        struct cmsghdr *cmsg;
        for (cmsg = CMSG_FIRSTHDR(&msg); cmsg != 0; cmsg = CMSG_NXTHDR(&msg, cmsg))
#else
        LPWSACMSGHDR cmsg;
        for (cmsg = WSA_CMSG_FIRSTHDR(&msg); cmsg != 0; cmsg = WSA_CMSG_NXTHDR(&msg, cmsg))
#endif
        {
            // IPv4 destination (IP_PKTINFO)
            // NOTE: may also be present for v4-mapped addresses in IPv6
            if (cmsg->cmsg_level == IPPROTO_IP && cmsg->cmsg_type == IP_PKTINFO && dst_len >= sizeof(struct sockaddr_in))
            {
#if !defined(__Windows__)
                struct in_pktinfo *info = (struct in_pktinfo *)CMSG_DATA(cmsg);
#else
                struct in_pktinfo *info = (struct in_pktinfo *)WSA_CMSG_DATA(cmsg);
#endif
                struct sockaddr_in *sa = (struct sockaddr_in *)dst;
                sa->sin_family = AF_INET;
                sa->sin_port = 0; // not provided by the posix api
#if !defined(__Windows__)
                sa->sin_addr = info->ipi_spec_dst;
#else
                sa->sin_addr = info->ipi_addr;
#endif
            }
            // IPv6 destination (IPV6_RECVPKTINFO)
            else if (cmsg->cmsg_level == IPPROTO_IPV6 && cmsg->cmsg_type == IPV6_PKTINFO && dst_len >= sizeof(struct sockaddr_in6))
            {
#if !defined(__Windows__)
                struct in6_pktinfo *info = (struct in6_pktinfo *)CMSG_DATA(cmsg);
#else
                struct in6_pktinfo *info = (struct in6_pktinfo *)WSA_CMSG_DATA(cmsg);

#endif
                struct sockaddr_in6 *sa = (struct sockaddr_in6 *)dst;
                sa->sin6_family = AF_INET6;
                sa->sin6_port = 0; // not provided by the posix api
#if !defined(__Windows__)
                sa->sin6_addr = info->ipi6_addr;
                sa->sin6_flowinfo = 0;
                sa->sin6_scope_id = 0;
#else
                sa->sin6_addr = info->ipi6_addr;
                sa->sin6_scope_id = info->ipi6_ifindex;
#endif
            }
#if defined(__Windows__)
            else if (cmsg->cmsg_level == IPPROTO_UDP && cmsg->cmsg_type == UDP_COALESCED_INFO)
            {
                printf("length=%u\n", (UINT16)*(PDWORD)WSA_CMSG_DATA(cmsg));
            }
#endif
        }
    }
#if !defined(__Windows__)
    return nb;
#else
    if (nResult == 0)
    {
        return ncounter;
    }
    else
    {
        printf("WSAGetLastError()=%d\n", WSAGetLastError());
        return -1;
    }
#endif
}