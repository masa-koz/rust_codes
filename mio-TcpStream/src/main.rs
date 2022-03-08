use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::thread;
use std::collections::HashMap;
use std::time::Duration;

const SERVER: mio::Token = mio::Token(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let receiver = thread::spawn(move || -> std::io::Result<()> {
        let mut listener = mio::net::TcpListener::bind("127.0.0.1:34255".parse().unwrap())?;
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut listener, mio::Token(0), mio::Interest::READABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let mut connections = HashMap::new();
        let mut unique_token = mio::Token(SERVER.0 + 1);

        let mut buf = [0; 512];
        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                match event.token() {
                    SERVER => loop {
                        let (mut conn, addr) = match listener.accept() {
                            Ok((conn, addr)) => (conn, addr),
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { break; }
                            Err(e) => {
                                return Err(e);
                            }
                        };
                        let token = next(&mut unique_token);
                        poll.registry()
                        .register(&mut conn, token, mio::Interest::READABLE)?;
                        connections.insert(token, conn);

                    }
                    token => {
                        let mut done = false;
                        if let Some(conn) = connections.get_mut(&token) {
                            loop {
                                let res = conn.try_io(|| {
                                    let buf_ptr = &mut buf as *mut _ as *mut _;
                                    let res =
                                        unsafe { libc::recv(conn.as_raw_fd(), buf_ptr, buf.len(), 0) };
                                    if res != -1 {
                                        Ok(res as usize)
                                    } else {
                                        Err(std::io::Error::last_os_error())
                                    }
                                });
                                match res {
                                    Ok(n) => {
                                        if n == 0 {
                                            done = true;
                                            break;
                                        }
                                        println!("read {:?} bytes", n);
                                    }
                                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => { break; }
                                    Err(e) => {
                                        return Err(e);
                                    }
                                }
                            }
                        }
                        if done {
                            if let Some(mut conn) = connections.remove(&token) {
                                poll.registry().deregister(&mut conn);
                            }
                        }
                    }
                }
            }
        }
    });

    let sender = thread::spawn(move || -> std::io::Result<()> {
        let mut stream = mio::net::TcpStream::connect("127.0.0.1:34255".parse().unwrap())?;
        let mut poll = mio::Poll::new()?;
        poll.registry()
            .register(&mut stream, mio::Token(0), mio::Interest::WRITABLE)?;
        let mut events = mio::Events::with_capacity(128);
        let buf = b"hello";

        loop {
            poll.poll(&mut events, None)?;
            for event in events.iter() {
                if event.is_writable() {
                    loop {
                        //let res = stream2.write(b"hello");
                        let res = stream.try_io(|| {
                            let buf_ptr = &buf as *const _ as *const _;
                            let res =
                                unsafe { libc::send(stream.as_raw_fd(), buf_ptr, buf.len(), 0) };
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

fn next(current: &mut mio::Token) -> mio::Token {
    let next = current.0;
    current.0 += 1;
    mio::Token(next)
}