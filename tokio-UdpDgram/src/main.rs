use std::time::Duration;

#[cfg(target_family = "unix")]
use std::os::unix::io::AsRawFd;
#[cfg(target_family = "windows")]
use std::os::windows::prelude::AsRawSocket;

#[tokio::main]
async fn main() {
    let task_recv = tokio::spawn(async move {
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:3456").await.unwrap();
        let mut buf = vec![0; 1500];

        loop {
            let _ = udp.readable().await;
            let res = udp.try_io(tokio::io::Interest::READABLE, || {
                let buf_ptr = &mut buf as *mut _ as *mut _;
                #[cfg(unix)]
                let res = unsafe { libc::recv(udp.as_raw_fd(), buf_ptr, buf.len(), 0) };
                #[cfg(windows)]
                let res = unsafe {
                    libc::recvfrom(
                        udp.as_raw_socket() as usize,
                        buf_ptr,
                        buf.len() as i32,
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                };
                if res < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(res as usize)
                }
            });
            match res {
                Ok(n) => {
                    println!("read {:?} bytes", n);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    println!("failed: {:?}", e);
                }
            }
        }
    });
    let task_send = tokio::spawn(async move {
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        udp.connect("127.0.0.1:3456").await.unwrap();
        let buf = b"hello";
        loop {
            let res = udp.try_io(tokio::io::Interest::WRITABLE, || {
                let buf_ptr = &buf as *const _ as *const _;
                #[cfg(unix)]
                let res =
                    unsafe { libc::send(udp.as_raw_fd(), buf_ptr, buf.len(), 0) };
                #[cfg(windows)]
                let res = unsafe {
                    libc::sendto(
                        udp.as_raw_socket() as usize,
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
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    println!("failed: {:?}", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let _ = tokio::join!(task_recv, task_send);
}
