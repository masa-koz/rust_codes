use os_socketaddr::OsSocketAddr;
use std::io;
use std::mem;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::os::raw::c_void;
use std::os::windows::io::{AsRawSocket, RawSocket};
use std::ptr;
use std::thread;
use std::time::Duration;
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetBestRoute2, NotifyRouteChange2, NotifyUnicastIpAddressChange, MIB_IPFORWARD_ROW2,
    MIB_NOTIFICATION_TYPE, MIB_UNICASTIPADDRESS_ROW,
};
use windows_sys::Win32::Networking::WinSock::{
    setsockopt, WSAIoctl, AF_INET, AF_UNSPEC, IN_PKTINFO, IPPROTO_IP, IP_PKTINFO, LPFN_WSARECVMSG,
    LPFN_WSASENDMSG, LPWSAOVERLAPPED_COMPLETION_ROUTINE, SIO_GET_EXTENSION_FUNCTION_POINTER,
    SOCKADDR_IN, SOCKADDR_INET, SOCKET, WSABUF, WSAMSG,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

extern "system" fn route_change_callback(
    _callercontext: *const c_void,
    row: *const MIB_IPFORWARD_ROW2,
    notificationtype: MIB_NOTIFICATION_TYPE,
) {
    if row != ptr::null() {
        let (prefix, prefix_len) = unsafe {
            let prefix = OsSocketAddr::from_raw_parts(
                &(*row).DestinationPrefix.Prefix as *const _ as *const u8,
                mem::size_of::<SOCKADDR_INET>(),
            )
            .into_addr()
            .unwrap()
            .ip();
            (prefix, (*row).DestinationPrefix.PrefixLength)
        };
        match notificationtype {
            0 => {
                eprintln!("[CHANGE] prefix: {:?}/{}", prefix, prefix_len);
            }
            1 => {
                eprintln!("[ADD] prefix: {:?}/{}", prefix, prefix_len);
            }
            2 => {
                eprintln!("[DELETE] prefix: {:?}/{}", prefix, prefix_len);
            }
            _ => {}
        }

        let dest_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(172, 29, 206, 88)), 0);
        let dest_addr: OsSocketAddr = dest_addr.into();
        let mut best_route: MIB_IPFORWARD_ROW2 = unsafe { mem::zeroed() };
        let mut best_src_addr = OsSocketAddr::new();
        let res = unsafe {
            GetBestRoute2(
                ptr::null(),
                0,
                ptr::null(),
                dest_addr.as_ptr() as *const _ as *const SOCKADDR_INET,
                0,
                &mut best_route,
                best_src_addr.as_mut_ptr() as *mut _ as *mut SOCKADDR_INET,
            )
        };
        if res == NO_ERROR as i32 {
            let nexthop = unsafe {
                OsSocketAddr::from_raw_parts(
                    &best_route.NextHop as *const _ as *const u8,
                    std::mem::size_of::<SOCKADDR_INET>(),
                )
            };
            eprintln!(" [BEST] nexthop: {:?}", nexthop);
            eprintln!(" [BEST] src_addr: {:?}", best_src_addr);
        }
    }
}

extern "system" fn address_change_callback(
    _callercontext: *const c_void,
    row: *const MIB_UNICASTIPADDRESS_ROW,
    notificationtype: MIB_NOTIFICATION_TYPE,
) {
    if row != ptr::null() {
        let address = unsafe {
            OsSocketAddr::from_raw_parts(
                &(*row).Address as *const _ as *const u8,
                mem::size_of::<SOCKADDR_INET>(),
            )
            .into_addr()
            .unwrap()
            .ip()
        };
        match notificationtype {
            0 => {
                eprintln!("[CHANGE] address: {:?}", address);
            }
            1 => {
                eprintln!("[ADD] address: {:?}", address);
            }
            2 => {
                eprintln!("[DELETE] address: {:?}", address);
            }
            _ => {}
        }
    }
}

#[repr(C)]
struct WSACMSGHDR {
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

fn WSA_CMSGHDR_ALIGN(length: usize) -> usize {
    length + mem::align_of::<WSACMSGHDR>() - 1 & !(mem::size_of::<WSACMSGHDR>() - 1)
}

fn WSA_CMSGDATA_ALIGN(length: usize) -> usize {
    length + mem::align_of::<usize>() - 1 & !(mem::size_of::<usize>() - 1)
}

unsafe fn WSA_CMSG_FIRSTHDR(msg: *const WSAMSG) -> *mut WSACMSGHDR {
    if (*msg).Control.len as usize >= mem::size_of::<WSACMSGHDR>() {
        (*msg).Control.buf as *mut WSACMSGHDR
    } else {
        0 as *mut WSACMSGHDR
    }
}

unsafe fn WSA_CMSG_NXTHDR(msg: *const WSAMSG, cmsg: *const WSACMSGHDR) -> *mut WSACMSGHDR {
    if cmsg == 0 as *mut WSACMSGHDR {
        return WSA_CMSG_FIRSTHDR(msg);
    }
    let next = (cmsg as usize + WSA_CMSGHDR_ALIGN((*cmsg).cmsg_len as usize)) as *mut WSACMSGHDR;
    let max = (*msg).Control.buf as usize + (*msg).Control.len as usize;
    if (next.offset(1)) as usize > max {
        0 as *mut WSACMSGHDR
    } else {
        next as *mut WSACMSGHDR
    }
}

unsafe fn WSA_CMSG_DATA(cmsg: *const WSACMSGHDR) -> *mut u8 {
    cmsg.offset(1) as *mut u8
}

#[inline]
fn WSA_CMSG_SPACE(length: usize) -> usize {
    WSA_CMSGDATA_ALIGN(mem::size_of::<WSACMSGHDR>() + WSA_CMSGHDR_ALIGN(length))
}

fn WSA_CMSG_LEN(length: usize) -> usize {
    WSA_CMSGDATA_ALIGN(mem::size_of::<WSACMSGHDR>() + length)
}

const WSAID_WSARECVMSG: GUID = GUID {
    data1: 0xf689d7c8,
    data2: 0x6f1f,
    data3: 0x436b,
    data4: [0x8a, 0x53, 0xe5, 0x4f, 0xe3, 0x51, 0xc3, 0x22],
};

type WSARecvMsg = unsafe extern "system" fn(
    s: SOCKET,
    lpMsg: *mut WSAMSG,
    lpdwnumberofbytesrecvd: *mut u32,
    lpoverlapped: *mut OVERLAPPED,
    lpcompletionroutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
) -> i32;

fn locate_wsarecvmsg(socket: RawSocket) -> io::Result<WSARecvMsg> {
    let mut func: LPFN_WSARECVMSG = None;
    let mut byte_len: u32 = 0;

    let res = unsafe {
        WSAIoctl(
            socket as _,
            SIO_GET_EXTENSION_FUNCTION_POINTER,
            &WSAID_WSARECVMSG as *const _ as *mut _,
            mem::size_of_val(&WSAID_WSARECVMSG) as _,
            &mut func as *const _ as *mut _,
            mem::size_of_val(&func) as _,
            &mut byte_len,
            ptr::null_mut(),
            None,
        )
    };
    if res != 0 {
        return Err(io::Error::last_os_error());
    }

    if byte_len as usize != mem::size_of::<LPFN_WSARECVMSG>() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Locating fn pointer to WSARecvMsg returned different expected bytes",
        ));
    }
    match func {
        None => Err(io::Error::new(
            io::ErrorKind::Other,
            "WSARecvMsg extension not foud",
        )),
        Some(extension) => Ok(extension),
    }
}

const WSAID_WSASENDMSG: GUID = GUID {
    data1: 0xa441e712,
    data2: 0x754f,
    data3: 0x43ca,
    data4: [0x84, 0xa7, 0x0d, 0xee, 0x44, 0xcf, 0x60, 0x6d],
};

type WSASendMsg = unsafe extern "system" fn(
    s: SOCKET,
    lpMsg: *const WSAMSG,
    dwFlags: u32,
    lpNumberOfBytesSent: *mut u32,
    lpOverlapped: *mut OVERLAPPED,
    lpCompletionRoutine: LPWSAOVERLAPPED_COMPLETION_ROUTINE,
) -> i32;

fn locate_wsasendmsg(socket: RawSocket) -> io::Result<WSASendMsg> {
    let mut func: LPFN_WSASENDMSG = None;
    let mut byte_len: u32 = 0;

    let r = unsafe {
        WSAIoctl(
            socket as _,
            SIO_GET_EXTENSION_FUNCTION_POINTER,
            &WSAID_WSASENDMSG as *const _ as *mut _,
            mem::size_of_val(&WSAID_WSASENDMSG) as _,
            &mut func as *const _ as *mut _,
            mem::size_of_val(&func) as _,
            &mut byte_len,
            ptr::null_mut(),
            None,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }

    if byte_len as usize != mem::size_of::<LPFN_WSASENDMSG>() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Locating fn pointer to WSASendMsg returned different expected bytes",
        ));
    }

    match func {
        None => Err(io::Error::new(
            io::ErrorKind::Other,
            "WSASendMsg extension not foud",
        )),
        Some(extension) => Ok(extension),
    }
}

fn main() -> io::Result<()> {
    let mut route_change_handle = 0;
    unsafe {
        NotifyRouteChange2(
            AF_UNSPEC as u16,
            Some(route_change_callback),
            ptr::null(),
            1,
            &mut route_change_handle,
        );
    }
    let mut address_change_handle = 0;
    unsafe {
        NotifyUnicastIpAddressChange(
            AF_UNSPEC as u16,
            Some(address_change_callback),
            ptr::null(),
            0,
            &mut address_change_handle,
        );
    }

    let receiver = thread::spawn(move || -> io::Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:8080")?;
        let optval = [1u32; 1];
        let res = unsafe {
            setsockopt(
                socket.as_raw_socket() as _,
                IPPROTO_IP as _,
                IP_PKTINFO as _,
                optval.as_ptr() as _,
                mem::size_of_val(&optval) as _,
            )
        };
        if res != 0 {
            eprintln!("{:?}", io::Error::last_os_error());
            return Err(io::Error::last_os_error());
        }

        let wsarecvmsg: WSARecvMsg = locate_wsarecvmsg(socket.as_raw_socket())?;

        let mut buf = vec![0u8; 1024];
        let mut control_buffer = [0; 128];
        let mut source: SOCKADDR_INET = unsafe { mem::zeroed() };

        loop {
            let mut data = WSABUF {
                buf: buf.as_mut_ptr(),
                len: buf.len() as _,
            };

            let control = WSABUF {
                buf: control_buffer.as_mut_ptr(),
                len: control_buffer.len() as _,
            };

            let mut wsa_msg = WSAMSG {
                name: &mut source as *mut _ as *mut _,
                namelen: mem::size_of_val(&source) as _,
                lpBuffers: &mut data,
                Control: control,
                dwBufferCount: 1,
                dwFlags: 0,
            };

            let mut read_bytes = 0;
            let res = {
                unsafe {
                    (wsarecvmsg)(
                        socket.as_raw_socket() as _,
                        &mut wsa_msg,
                        &mut read_bytes,
                        ptr::null_mut(),
                        None,
                    )
                }
            };
            if res == 0 {
                let destination_address = unsafe {
                    let cmsg = WSA_CMSG_FIRSTHDR(&wsa_msg);
                    if cmsg != ptr::null_mut()
                        && (*cmsg).cmsg_level == IPPROTO_IP as _
                        && (*cmsg).cmsg_type == IP_PKTINFO as _
                        && (*cmsg).cmsg_len >= WSA_CMSG_LEN(mem::size_of::<IN_PKTINFO>())
                    {
                        let info: IN_PKTINFO = ptr::read(WSA_CMSG_DATA(cmsg) as *const _);
                        let destination = SOCKADDR_IN {
                            sin_family: AF_INET as _,
                            sin_addr: info.ipi_addr,
                            sin_port: 8080u16.to_be(),
                            sin_zero: [0u8; 8],
                        };
                        socket2::SockAddr::from_raw_parts(
                            &destination as *const _ as *const _,
                            mem::size_of_val(&destination) as _,
                        )
                        .as_std()
                    } else {
                        None
                    }
                };
                let source_address = unsafe {
                    socket2::SockAddr::from_raw_parts(
                        &source as *const _ as *const _,
                        mem::size_of_val(&source) as _,
                    )
                }
                .as_std();
                eprintln!(
                    "read_bytes={}, destination={:?}, source={:?}",
                    read_bytes, destination_address, source_address
                );
            }
        }

        Ok(())
    });

    let sender = thread::spawn(move || -> io::Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        let wsasendmsg: WSASendMsg = locate_wsasendmsg(socket.as_raw_socket())?;

        let buf = "hello";
        let mut data = WSABUF {
            buf: buf.as_ptr() as *mut _,
            len: buf.len() as _,
        };

        let destination_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 213, 165)), 8080);
        let destination = socket2::SockAddr::from(destination_address);

        let source_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 208, 2)), 0);
        let source = socket2::SockAddr::from(source_address);
        let source: SOCKADDR_IN = unsafe { ptr::read(source.as_ptr() as _) };
        let info: IN_PKTINFO = IN_PKTINFO {
            ipi_addr: source.sin_addr,
            ipi_ifindex: 0,
        };

        let mut control_buffer = [0; 128];
        let cmsg = WSACMSGHDR {
            cmsg_len: WSA_CMSG_LEN(mem::size_of::<IN_PKTINFO>()),
            cmsg_level: IPPROTO_IP as _,
            cmsg_type: IP_PKTINFO as _,
        };
        unsafe {
            ptr::copy(
                &cmsg as *const _ as *const _,
                control_buffer.as_mut_ptr(),
                mem::size_of::<WSACMSGHDR>(),
            );
            ptr::copy(
                &info as *const _ as *const _,
                WSA_CMSG_DATA(control_buffer.as_mut_ptr() as _),
                mem::size_of::<IN_PKTINFO>(),
            )
        };
        let control = WSABUF {
            buf: control_buffer.as_mut_ptr(),
            len: WSA_CMSG_LEN(mem::size_of::<IN_PKTINFO>()) as _,
        };

        let mut wsa_msg = WSAMSG {
            name: destination.as_ptr() as *mut _,
            namelen: destination.len(),
            lpBuffers: &mut data,
            Control: control,
            dwBufferCount: 1,
            dwFlags: 0,
        };

        loop {
            let mut sent_bytes = 0;
            let res = unsafe {
                (wsasendmsg)(
                    socket.as_raw_socket() as _,
                    &mut wsa_msg,
                    0,
                    &mut sent_bytes,
                    ptr::null_mut(),
                    None,
                )
            };
            if res != 0 {
                eprintln!("{:?}", io::Error::last_os_error());
            } else {
                eprintln!("sent_bytes={}", sent_bytes);
            }
            thread::sleep(Duration::from_secs(1));
        }
        Ok(())
    });

    let _ = receiver.join().expect("The receiver thread has panicked");
    let _ = sender.join().expect("The sender thread has panicked");

    Ok(())
}
