use std::io;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = thread::spawn(move || -> io::Result<()> {
        let mut dgram = mio::net::UdpSocket::bind("127.0.0.1:34567".parse().unwrap())?;
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut dgram, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);

        let mut buf = [0; 512];
        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
                        let res = dgram.try_io(|| {
                            let buf_ptr = &mut buf as *mut _ as *mut _;
                            #[cfg(unix)]
                            let res =
                                unsafe { libc::recv(dgram.as_raw_fd(), buf_ptr, buf.len(), 0) };
                            #[cfg(windows)]
                            let res = unsafe {
                                libc::recvfrom(
                                    dgram.as_raw_socket() as usize,
                                    buf_ptr,
                                    buf.len() as i32,
                                    0,
                                    std::ptr::null_mut(),
                                    std::ptr::null_mut(),
                                )
                            };
                            if res != -1 {
                                Ok(res as usize)
                            } else {
                                Err(std::io::Error::last_os_error())
                            }
                        });
                        match res {
                            Ok(n) => {
                                println!("read {:?} bytes", n);
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(e) => {
                                println!("failed: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    });

    let sender = thread::spawn(move || -> io::Result<()> {
        let mut dgram = mio::net::UdpSocket::bind("127.0.0.1:0".parse().unwrap())?;
        dgram.connect("127.0.0.1:34567".parse().unwrap()).unwrap();
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut dgram, mio::Token(0), mio::Interest::WRITABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let buf = b"hello";

        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_writable() {
                    loop {
                        //let res = stream2.write(b"hello");
                        let res = dgram.try_io(|| {
                            let buf_ptr = &buf as *const _ as *const _;
                            #[cfg(unix)]
                            let res =
                                unsafe { libc::send(dgram.as_raw_fd(), buf_ptr, buf.len(), 0) };
                            #[cfg(windows)]
                            let res = unsafe {
                                libc::sendto(
                                    dgram.as_raw_socket() as usize,
                                    buf_ptr,
                                    buf.len() as i32,
                                    0,
                                    std::ptr::null(),
                                    0,
                                )
                            };
                            if res != -1 {
                                Ok(res as usize)
                            } else {
                                Err(std::io::Error::last_os_error())
                            }
                        });
                        match res {
                            Ok(n) => {
                                println!("write {:?} bytes", n);
                            }
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                break;
                            }
                            Err(e) => {
                                println!("failed: {:?}", e);
                            }
                        }
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
        }
    });

    let res = receiver.join();
    println!("{:?}", res);
    let res = sender.join();
    println!("{:?}", res);
    Ok(())
}
