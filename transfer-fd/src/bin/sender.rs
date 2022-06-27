#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
    use nix::unistd::{close, pipe, write};
    use std::env;
    use std::io::IoSlice;
    use std::os::unix::net::UnixDatagram;
    use std::os::unix::prelude::AsRawFd;
    use std::path::PathBuf;

    let (r, w) = pipe().expect("pipe failed");
    {
        let tempdir = tempfile::tempdir().unwrap();
        let local_name = tempdir.path().join("sock");
        let remote_name = PathBuf::from(env::args().nth(1).expect("no sockname"));

        let socket = UnixDatagram::bind(&local_name).expect("bind failed");
        socket.connect(remote_name).expect("connect failed");

        let iov = [IoSlice::new(b"hello")];
        let fds = [r];
        let cmsg = ControlMessage::ScmRights(&fds);
        sendmsg::<()>(socket.as_raw_fd(), &iov, &[cmsg], MsgFlags::empty(), None)
            .expect("sendmsg failed");
        close(r).expect("close failed");
    }

    loop {
        write(w, b"hello").expect("write failed");
    }
}

#[cfg(windows)]
fn main() {
    println!("Not supported on Windows");
}
