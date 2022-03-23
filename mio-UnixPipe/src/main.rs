#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::io::AsRawFd;
    use std::thread;
    use std::time::Duration;

    let (mut sender, mut receiver) = mio::unix::pipe::new()?;

    let receiver_thr = thread::spawn(move || -> std::io::Result<()> {
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut receiver, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);

        let mut buf = [0; 512];
        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
                        let res = receiver.try_io(|| {
                            let buf_ptr = &mut buf as *mut _ as *mut _;
                            let res =
                                unsafe { libc::read(receiver.as_raw_fd(), buf_ptr, buf.len()) };
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
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
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

    let sender_thr = thread::spawn(move || -> std::io::Result<()> {
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut sender, mio::Token(0), mio::Interest::WRITABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let buf = b"hello";

        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_writable() {
                    loop {
                        //let res = stream2.write(b"hello");
                        let res = sender.try_io(|| {
                            let buf_ptr = &buf as *const _ as *const _;
                            let res =
                                unsafe { libc::write(sender.as_raw_fd(), buf_ptr, buf.len()) };
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
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
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

    let res = receiver_thr.join();
    println!("{:?}", res);
    let res = sender_thr.join();
    println!("{:?}", res);
    Ok(())
}

#[cfg(windows)]
fn main() {
    println!("Not supported on Windows");
}

