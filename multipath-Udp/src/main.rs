use std::net::ToSocketAddrs;

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

    let socket = if addr.is_ipv4() {
        std::net::UdpSocket::bind("0.0.0.0:0")?
    } else {
        std::net::UdpSocket::bind("[::]:0")?
    };

    #[cfg(windows)]
    if matches.is_present("if_index") {
        let if_index: u32 = matches.value_of_t("if_index").expect("validated");

        use os_socketaddr::OsSocketAddr;
        use windows_sys::Win32::Foundation::NO_ERROR;
        use windows_sys::Win32::NetworkManagement::IpHelper::{
            GetBestRoute2, IP_ADDRESS_PREFIX, MIB_IPFORWARD_ROW2, NET_LUID_LH,
        };
        use windows_sys::Win32::Networking::WinSock::{SOCKADDR_IN6, SOCKADDR_INET};

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

        let res: u32 = unsafe {
            GetBestRoute2(
                std::ptr::null(),
                if_index,
                std::ptr::null(),
                destinationaddress,
                0,
                &mut bestroute,
                &mut bestsourceaddress,
            ) as u32
        };
        if res != NO_ERROR {
            eprintln!("No route to {} via IF {}", host, if_index);
            return Ok(());
        }
        let nexthop = unsafe {
            OsSocketAddr::from_raw_parts(
                &bestroute.NextHop as *const _ as *const _,
                std::mem::size_of::<SOCKADDR_IN6>(),
            )
        }
        .into_addr();
        println!("nexthop: {:?}", nexthop);

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
    }
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
