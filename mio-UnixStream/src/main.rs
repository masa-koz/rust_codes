use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (mut stream1, mut stream2) = mio::net::UnixStream::pair()?;

    let receiver = thread::spawn(move || -> std::io::Result<()> {
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut stream1, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);

        let mut buf = [0; 512];
        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_readable() {
                    loop {
                        let res = stream1.try_io(|| {
                            let buf_ptr = &mut buf as *mut _ as *mut _;
                            let res =
                                unsafe { libc::recv(stream1.as_raw_fd(), buf_ptr, buf.len(), 0) };
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
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { break; }
                            Err(e) => {
                                println!("failed: {:?}", e);
                            }
                        }
                    }
                }
            }
        }
    });

    let sender = thread::spawn(move || -> std::io::Result<()> {
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut stream2, mio::Token(0), mio::Interest::WRITABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let buf = b"hello";

        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_writable() {
                    loop {
                        //let res = stream2.write(b"hello");
                        let res = stream2.try_io(|| {
                            let buf_ptr = &buf as *const _ as *const _;
                            let res =
                                unsafe { libc::send(stream2.as_raw_fd(), buf_ptr, buf.len(), 0) };
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
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { break; }
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
