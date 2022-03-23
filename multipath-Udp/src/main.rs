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

    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;

    #[cfg(windows)]
    if matches.is_present("if_index") {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{setsockopt, IPPROTO_IP, IP_UNICAST_IF};
        let if_index: u32 = matches.value_of_t("if_index").expect("validated");
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
