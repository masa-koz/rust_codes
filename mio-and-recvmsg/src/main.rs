use os_socketaddr::OsSocketAddr;
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;

#[cfg(target_family = "unix")]
use libc::sockaddr;
#[cfg(target_family = "unix")]
use libc::socklen_t;

#[cfg(target_family = "windows")]
use std::os::windows::prelude::AsRawSocket;
#[cfg(target_family = "windows")]
use winapi::shared::ws2def::SOCKADDR as sockaddr;
#[cfg(target_family = "windows")]
use winapi::um::ws2tcpip::socklen_t;

#[link(name = "rust_recvmsg", kind = "static")]
extern "C" {
    fn recvmsg_for_quic(
        sock: libc::intptr_t,
        buf: *mut u8,
        buf_len: libc::size_t,
        flags: libc::c_int,
        src: *mut sockaddr,
        src_len: socklen_t,
        dst: *mut sockaddr,
        dst_len: socklen_t,
    ) -> libc::ssize_t;
}

fn main() {
    let receiver = thread::spawn(move || -> std::io::Result<()> {
        let mut poll = mio::Poll::new()?;
        let udp = std::net::UdpSocket::bind("127.0.0.1:3456")?;
        udp.set_nonblocking(true)?;
        let mut udp = mio::net::UdpSocket::from_std(udp);
        poll.registry()
            .register(&mut udp, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let mut buf = vec![0; 4096];

        loop {
            println!("poll");
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
                        /*
                        match udp.recv_from(&mut buf) {
                            Ok((len, from)) => {
                                println!("len: {}, from: {}", len, from);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(e) => {
                                println!("recv_from failed: {:?}", e);
                                continue;
                            }
                        }
                        */
                        let mut from = OsSocketAddr::new();
                        let mut to = OsSocketAddr::new();
                        let res = udp.do_io(|| -> std::io::Result<(usize, Option<SocketAddr>, Option<SocketAddr>)> {
                            let res = unsafe {
                                recvmsg_for_quic(
                                    udp.as_raw_socket() as isize,
                                    buf.as_mut_ptr(),
                                    buf.len(),
                                    0,
                                    from.as_mut_ptr(),
                                    from.capacity(),
                                    to.as_mut_ptr(),
                                    to.capacity(),
                                )
                            };
                            if res < 0 {
                                Err(std::io::Error::last_os_error())
                            } else {
                                Ok((res as usize, from.into(), to.into()))
                            }
                        });
                        match res {
                            Ok((len, from, to)) => {
                                println!("len: {}, from: {:?}, to: {:?}", len, from, to);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { break; }
                            Err(e) => {
                                println!("failed: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    });
    let sender = thread::spawn(move || -> std::io::Result<()> {
        let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        loop {
            udp.send_to(&[0; 10], "127.0.0.1:3456")?;
            thread::sleep(Duration::from_secs(1));
        }
        Ok(())
    });

    let _ = receiver.join().expect("The sender thread has panicked");
    let _ = sender.join().expect("The sender thread has panicked");
}
