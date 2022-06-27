#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use nix::cmsg_space;
    use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
    use nix::unistd::read;
    use std::io::IoSliceMut;
    use std::os::unix::io::{AsRawFd, RawFd};
    use std::os::unix::net::UnixDatagram;

    let received_fd: Option<RawFd> = {
        let tempdir = tempfile::tempdir().unwrap();
        let sockname = tempdir.path().join("sock");

        let socket = UnixDatagram::bind(&sockname).expect("bind failed");

        println!("{}", sockname.to_str().unwrap());

        let mut buf = [0u8; 1024];
        let mut iov = [IoSliceMut::new(&mut buf[..])];
        let mut cmsgspace = cmsg_space!([RawFd; 1]);

        let msg = recvmsg::<()>(
            socket.as_raw_fd(),
            &mut iov,
            Some(&mut cmsgspace),
            MsgFlags::empty(),
        )
        .expect("recvmsg failed");

        if let Some(ControlMessageOwned::ScmRights(fds)) = msg.cmsgs().next() {
            Some(fds[0])
        } else {
            None
        }
    };

    if let Some(received_fd) = received_fd {
        let mut buf = [0u8; 1024];
        loop {
            let len = read(received_fd, &mut buf).expect("read failed");
            if len == 0 {
                break;
            }
            println!("len={}", len);
        }
    }
    Ok(())
}

#[cfg(windows)]
fn main() {
    println!("Not supported on Windows");
}
