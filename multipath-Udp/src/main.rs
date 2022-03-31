use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};

fn main() -> std::io::Result<()> {
    let app = clap::command!()
        .arg(
            clap::arg!([HOST])
                .help("Specify HOST to which you send packet")
                .required(true),
        )
        .arg(
            clap::arg!(-p --port <PORT>)
                .help("Specify PORT to which you send packet")
                .required(false)
                .default_value("4567")
                .validator(|s| s.parse::<usize>()),
        )
        .arg(clap::arg!(--ipv4  "Use IPv4"))
        .arg(clap::arg!(--ipv6  "Use IPv6"))
        .group(
            clap::ArgGroup::new("ip_version")
                .required(true)
                .args(&["ipv4", "ipv6"]),
        );

    #[cfg(windows)]
    let app = app.arg(
        clap::arg!(-i --if_index <IFIndex>)
            .help("Specify If's index from which you send packet")
            .required(false)
            .validator(|s| s.parse::<u32>()),
    );

    let matches = app.get_matches();

    let host = matches.value_of("HOST").expect("'HOST' is required.");
    let port: usize = matches
        .value_of_t("port")
        .expect("'PORT' has default value.");

    let addrs_iter = format!("{}:{}", host, port).to_socket_addrs()?;
    let addr = addrs_iter
        .filter(|addr| {
            (addr.is_ipv4() && matches.is_present("ipv4"))
                || (addr.is_ipv6() && matches.is_present("ipv6"))
        })
        .next();

    let addr = if let Some(addr) = addr {
        addr
    } else {
        eprintln!("No address: {}", host);
        return Ok(());
    };

    let mut interfaces = HashMap::new();

    #[cfg(windows)]
    if matches.is_present("if_index") {
        let if_index: u32 = matches.value_of_t("if_index").expect("validated");
        interfaces.insert(if_index, ());
    } else {
        use windows_sys::Win32::Foundation::NO_ERROR;
        use windows_sys::Win32::NetworkManagement::IpHelper::{
            FreeMibTable, GetUnicastIpAddressTable, AF_UNSPEC, MIB_UNICASTIPADDRESS_ROW,
            MIB_UNICASTIPADDRESS_TABLE,
        };
        unsafe {
            let mut table: *mut MIB_UNICASTIPADDRESS_TABLE = std::ptr::null_mut();
            let res = GetUnicastIpAddressTable(AF_UNSPEC as u16, &mut table) as u32;
            if res == NO_ERROR {
                for i in 0..(*table).NumEntries as isize {
                    let row = *(&(*table).Table[0] as *const MIB_UNICASTIPADDRESS_ROW).offset(i);
                    interfaces.insert(row.InterfaceIndex, ());
                    /*
                    use os_socketaddr::OsSocketAddr;
                    use windows_sys::Win32::Networking::WinSock::SOCKADDR_IN6;

                    eprintln!("InterfaceIndex: {}", row.InterfaceIndex);
                    let address = OsSocketAddr::from_raw_parts(
                        &row.Address as *const _ as *const _,
                        std::mem::size_of::<SOCKADDR_IN6>(),
                    )
                    .into_addr();
                    eprintln!("address: {:?}", address);
                    */
                }
            }
            if !table.is_null() {
                FreeMibTable(table as *const _);
            }
        }
    }

    for if_index in interfaces.into_keys() {
        match send_hello(addr, if_index) {
            Ok(_) => {
                eprintln!("Send \"hello\" to {:?} via IF {}", addr, if_index);
            }
            Err(e) => {
                eprintln!(
                    "Failed to send \"hello\" to {:?} via IF {}: {:?}",
                    addr, if_index, e
                );
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn send_hello(addr: SocketAddr, _if_index: u32) -> std::io::Result<()> {
    let socket = if addr.is_ipv4() {
        std::net::UdpSocket::bind("0.0.0.0:0")?
    } else {
        std::net::UdpSocket::bind("[::]:0")?
    };

    match socket.send_to(b"hello", addr) {
        Ok(len) => {
            eprintln!("Send {} bytes to {:?}", len, addr);
        }
        Err(e) => {
            eprintln!("Failed to send to {:?}: {:?}", addr, e);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn send_hello(addr: SocketAddr, if_index: u32) -> std::io::Result<usize> {
    use os_socketaddr::OsSocketAddr;
    use windows_sys::Win32::Foundation::NO_ERROR;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetBestRoute2, GetIpPathEntry, AF_UNSPEC, IP_ADDRESS_PREFIX, MIB_IPFORWARD_ROW2,
        MIB_IPPATH_ROW, MIB_IPPATH_ROW_0, NET_LUID_LH,
    };
    use windows_sys::Win32::Networking::WinSock::SOCKADDR_INET;

    let destinationaddress: OsSocketAddr = addr.into();
    let destinationaddress = destinationaddress.as_ptr() as *const _ as *const _;

    let mut bestroute = MIB_IPFORWARD_ROW2 {
        InterfaceLuid: NET_LUID_LH { Value: 0 },
        InterfaceIndex: 0,
        DestinationPrefix: IP_ADDRESS_PREFIX {
            Prefix: SOCKADDR_INET { si_family: 0 },
            PrefixLength: 0,
        },
        NextHop: SOCKADDR_INET { si_family: 0 },
        SitePrefixLength: 0,
        ValidLifetime: 0,
        PreferredLifetime: 0,
        Metric: 0,
        Protocol: 0,
        Loopback: 0,
        AutoconfigureAddress: 0,
        Publish: 0,
        Immortal: 0,
        Age: 0,
        Origin: 0,
    };
    let mut bestsourceaddress = SOCKADDR_INET { si_family: 0 };

    let res = unsafe {
        GetBestRoute2(
            std::ptr::null(),
            if_index,
            std::ptr::null(),
            destinationaddress,
            0,
            &mut bestroute,
            &mut bestsourceaddress,
        )
    };
    if res as u32 != NO_ERROR {
        eprintln!("No route to {} via IF {}", addr, if_index);
        return Err(std::io::Error::from_raw_os_error(res));
    }
    /*
    use windows_sys::Win32::Networking::WinSock::SOCKADDR_IN6;
    let nexthop = unsafe {
        OsSocketAddr::from_raw_parts(
            &bestroute.NextHop as *const _ as *const _,
            std::mem::size_of::<SOCKADDR_IN6>(),
        )
    }
    .into_addr();
    eprintln!("nexthop: {:?}", nexthop);

    let bestsourceaddress = unsafe {
        OsSocketAddr::from_raw_parts(
            &bestsourceaddress as *const _ as *const _,
            std::mem::size_of::<SOCKADDR_IN6>(),
        )
    }
    .into_addr();
    eprintln!("bestsourceaddress: {:?}", bestsourceaddress);
    */

    let mut pathentry = MIB_IPPATH_ROW {
        Source: SOCKADDR_INET {
            si_family: AF_UNSPEC as u16,
        },
        Destination: SOCKADDR_INET {
            si_family: AF_UNSPEC as u16,
        },
        InterfaceLuid: NET_LUID_LH { Value: 0 },
        InterfaceIndex: 0,
        CurrentNextHop: SOCKADDR_INET {
            si_family: AF_UNSPEC as u16,
        },
        PathMtu: 0,
        RttMean: 0,
        RttDeviation: 0,
        Anonymous: MIB_IPPATH_ROW_0 { LastReachable: 0 },
        IsReachable: 0,
        LinkTransmitSpeed: 0,
        LinkReceiveSpeed: 0,
    };
    pathentry.Destination = unsafe { *destinationaddress };
    pathentry.InterfaceIndex = if_index;

    let res = unsafe { GetIpPathEntry(&mut pathentry) };
    if res as u32 != NO_ERROR {
        eprintln!("No entry of path to {} via IF {}", addr, if_index);
        return Err(std::io::Error::from_raw_os_error(res));
    }
    eprintln!("IsReachable: {}", pathentry.IsReachable);
    eprintln!("PathMtu: {}", pathentry.PathMtu);

    let socket = if addr.is_ipv4() {
        std::net::UdpSocket::bind("0.0.0.0:0")?
    } else {
        std::net::UdpSocket::bind("[::]:0")?
    };

    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        setsockopt, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, IP_UNICAST_IF,
    };

    if addr.is_ipv4() {
        let optval: [u32; 1] = [if_index.to_be(); 1];
        let optlen: i32 = std::mem::size_of_val(&optval).try_into().unwrap();
        unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                IPPROTO_IP as i32,
                IP_UNICAST_IF as i32,
                optval.as_ptr() as *const u8,
                optlen,
            );
        }
    } else {
        let optval: [u32; 1] = [if_index; 1];
        let optlen: i32 = std::mem::size_of_val(&optval).try_into().unwrap();
        unsafe {
            setsockopt(
                socket.as_raw_socket() as usize,
                IPPROTO_IPV6 as i32,
                IPV6_UNICAST_IF as i32,
                optval.as_ptr() as *const u8,
                optlen,
            );
        }
    }

    socket.send_to(b"hello", addr)
}
