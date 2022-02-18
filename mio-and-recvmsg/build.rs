extern crate cc;

#[cfg(target_family = "unix")]
fn main() {
    cc::Build::new()
        .file("src/recvmsg.c")
        .compile("librust_recvmsg.a");
}

#[cfg(target_family = "windows")]
fn main() {
    cc::Build::new()
        .file("src/recvmsg.c")
        .define("__Windows__", None)
        .compile("librust_recvmsg.a");
}