use os_socketaddr::OsSocketAddr;
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
        sock: libc::SOCKET,
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
        poll.registry().register(&mut udp, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let mut buf = vec![0; 4096];

        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
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
