use std::boxed::Box;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures::future::{poll_fn, FutureExt};
use futures::{SinkExt, Stream, StreamExt};
use futures_util::ready;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{PollSendError, PollSender};

struct QuicRecvStream {
    receiver: mpsc::Receiver<Bytes>,
    local_storage: Option<Bytes>,
    sender: PollSender<(Request, oneshot::Sender<Response>)>,
    stream_id: u64,
    cmd_pending: HashMap<usize, Command>,
}

impl<'a> AsyncRead for QuicRecvStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();

        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }

        'outer: loop {
            while let Some(mut bytes) = me.local_storage.take() {
                println!("buf's len={}, bytes's len={}", buf.remaining(), bytes.len());
                let len = buf.remaining().min(bytes.len());
                buf.put_slice(&bytes.split_to(len)[..]);
                println!("New bytes's len={}", bytes.len());
                if !bytes.is_empty() {
                    me.local_storage = Some(bytes);
                }

                if buf.remaining() == 0 {
                    break 'outer;
                }
            }

            let bytes = match ready!(me.receiver.poll_recv(cx)) {
                Some(bytes) => bytes,
                None => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "oneshot recv failed",
                    )));
                }
            };
            println!("arrived bytes's len={}", bytes.len());
            me.local_storage = Some(bytes);
        }
        Poll::Ready(Ok(()))
    }
}

struct QuicSendStream {
    sender: PollSender<(Request, oneshot::Sender<Response>)>,
    stream_id: u64,
    cmd_pending: HashMap<usize, Command>,
}

impl<'a> AsyncWrite for QuicSendStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let me = self.get_mut();

        let key: usize = std::ptr::addr_of!(cx) as usize;
        if !me.cmd_pending.contains_key(&key) {
            let mut bytes_mut = BytesMut::new();
            bytes_mut.extend_from_slice(buf);
            let msg = Request::Write {
                buf: bytes_mut.freeze(),
            };
            me.cmd_pending
                .insert(key, Command::State0 { msg: Some(msg) });
        }
        let cmd = me.cmd_pending.get_mut(&key).unwrap();

        let res = match cmd.poll_execute(&mut me.sender, cx) {
            Poll::Ready(Ok(Response::Write { written })) => Poll::Ready(Ok(written)),
            Poll::Ready(Ok(_)) => {
                panic!("Invalid response!");
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "oneshot recv failed",
            ))),
            Poll::Pending => Poll::Pending,
        };
        if !res.is_pending() {
            me.cmd_pending.remove(&key);
        }
        res
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct QuicDgram {
    write: QuicWriteDgram,
    read: QuicReadDgram,
}

struct QuicWriteDgram {
    cmd_sender: PollSender<(Request, oneshot::Sender<Response>)>,
    cmd_pending: HashMap<usize, Command>,
    write_sender: PollSender<Bytes>,
}

impl QuicWriteDgram {
    fn poll_write_chunk(
        &mut self,
        cx: &mut Context<'_>,
        buf: &Bytes,
    ) -> Poll<std::result::Result<(), WriteDgramError>> {
        match ready!(self.poll_ready(cx)) {
            Ok(()) => {
                if let Err(_) = self.write_sender.send_item(buf.clone()) {
                    return Poll::Ready(Err(WriteDgramError::ConnectionClosed));
                }
                return Poll::Ready(Ok(()));
            }
            Err(e) => {
                return Poll::Ready(Err(WriteDgramError::ConnectionClosed));
            }
        }
    }

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), WriteDgramError>> {
        let key: usize = std::ptr::addr_of!(cx) as usize;

        if !self.cmd_pending.contains_key(&key) {
            if let Some(write_sender) = self.write_sender.get_ref() {
                if write_sender.capacity() > 0 {
                    self.cmd_pending.insert(key, Command::Terminated);
                } else {
                    let msg = Request::WriteDgramReady;
                    self.cmd_pending
                        .insert(key, Command::State0 { msg: Some(msg) });
                }
            } else {
                return Poll::Ready(Err(WriteDgramError::ConnectionClosed));
            }
        }

        let cmd = self.cmd_pending.get_mut(&key).unwrap();

        if !cmd.is_terminated() {
            match ready!(cmd.poll_execute(&mut self.cmd_sender, cx)) {
                Ok(Response::WriteDgramReady) => {}
                Ok(_) => {
                    panic!("Invalid response!");
                }
                Err(e) => return Poll::Ready(Err(WriteDgramError::ConnectionClosed)),
            }
        }

        let res = match ready!(self.write_sender.poll_reserve(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(WriteDgramError::ConnectionClosed)),
        };

        self.cmd_pending.remove(&key);
        res
    }

    fn poll_flush(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), WriteDgramError>> {
        let key: usize = std::ptr::addr_of!(cx) as usize;
        if !self.cmd_pending.contains_key(&key) {
            let msg = Request::WriteDgramFlush;
            self.cmd_pending
                .insert(key, Command::State0 { msg: Some(msg) });
        }
        let cmd = self.cmd_pending.get_mut(&key).unwrap();

        let res = match ready!(cmd.poll_execute(&mut self.cmd_sender, cx)) {
            Ok(Response::WriteDgramFlush) => Poll::Ready(Ok(())),
            Ok(_) => {
                panic!("Invalid response!");
            }
            Err(e) => Poll::Ready(Err(WriteDgramError::ConnectionClosed)),
        };
        self.cmd_pending.remove(&key);
        res
    }

    fn poll_close(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), WriteDgramError>> {
        let res = ready!(self.poll_flush(cx));
        if res.is_err() {
            return Poll::Ready(res);
        }
        self.write_sender.close();
        Poll::Ready(Ok(()))
    }

}

struct QuicReadDgram {
    req_sender: PollSender<(Request, oneshot::Sender<Response>)>,
    read_receiver: mpsc::Receiver<Bytes>,
}

impl QuicReadDgram {
    async fn read_chunk(&mut self) -> std::result::Result<Bytes, ReadDgramError> {
        ReadChunk { read_dgram: self }.await
    }

    fn poll_read_chunk(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<std::result::Result<Bytes, ReadDgramError>> {
        match self.read_receiver.poll_recv(cx) {
            Poll::Ready(Some(bytes)) => {
                return Poll::Ready(Ok(bytes));
            }
            Poll::Ready(None) => {
                return Poll::Ready(Err(ReadDgramError::ConnectionClosed));
            }
            Poll::Ready(Some(_)) => {
                panic!("Invalid response!");
            }
            Poll::Pending => {
                return Poll::Pending;
            }
        }
    }
}

impl Stream for QuicReadDgram {
    type Item = std::result::Result<Bytes, ReadDgramError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let me = self.get_mut();

        Poll::Ready(Some(ready!(me.poll_read_chunk(cx))))
    }
}

struct ReadChunk<'a> {
    read_dgram: &'a mut QuicReadDgram,
}

impl<'a> Future for ReadChunk<'a> {
    type Output = std::result::Result<Bytes, ReadDgramError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let me = self.get_mut();

        me.read_dgram.poll_read_chunk(cx)
    }
}

#[derive(Debug)]
enum WriteDgramError {
    ConnectionClosed,
}

#[derive(Debug)]
enum ReadDgramError {
    ConnectionClosed,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Request {
    Read { len: usize },
    Write { buf: Bytes },
    WriteDgramFlush,
    WriteDgramReady,
}

#[derive(Debug)]
enum Response {
    Read { buf: Bytes },
    Write { written: usize },
    ReadDgram { bytes: Bytes },
    WriteDgramFlush,
    WriteDgramReady,
}

#[derive(Debug)]
enum RequestError {
    NoRequest,
    SendError(PollSendError<(Request, oneshot::Sender<Response>)>),
    RecvError(oneshot::error::RecvError),
}

enum Command {
    State0 { msg: Option<Request> },
    State1 { recv: oneshot::Receiver<Response> },
    Terminated,
}

impl Command {
    fn poll_execute(
        &mut self,
        sender: &mut PollSender<(Request, oneshot::Sender<Response>)>,
        cx: &mut Context,
    ) -> Poll<std::result::Result<Response, RequestError>> {
        loop {
            match self {
                Command::State0 { ref mut msg } => {
                    println!("State0");
                    match ready!(sender.poll_reserve(cx)) {
                        Ok(()) => {
                            if msg.is_none() {
                                return Poll::Ready(Err(RequestError::NoRequest));
                            }
                            let msg = msg.take().unwrap();
                            let (send, mut recv) = oneshot::channel();
                            if let Err(e) = sender.send_item((msg, send)) {
                                return Poll::Ready(Err(RequestError::SendError(e)));
                            }
                            *self = Command::State1 { recv: recv };
                        }
                        Err(e) => {
                            return Poll::Ready(Err(RequestError::SendError(e)));
                        }
                    }
                }
                Command::State1 { ref mut recv } => {
                    println!("State1");
                    match ready!(recv.poll_unpin(cx)) {
                        Ok(response) => {
                            *self = Command::Terminated;
                            return Poll::Ready(Ok(response));
                        }
                        Err(e) => {
                            return Poll::Ready(Err(RequestError::RecvError(e)));
                        }
                    }
                }
                Command::Terminated => {
                    panic!("future polled after completion");
                }
            }
        }
    }

    fn is_terminated(&self) -> bool {
        if let Command::Terminated = self {
            true
        } else {
            false
        }
    }
}

#[tokio::main]
async fn main() {
    let (sender, mut receiver) = mpsc::channel::<(Request, oneshot::Sender<Response>)>(128);
    let (dgram_read_sender, dgram_read_reciver) = mpsc::channel(1);
    let (dgram_write_sender, mut dgram_write_receiver) = mpsc::channel::<Bytes>(1);
    let (stream_read_sender, stream_read_receiver) = mpsc::channel(1);

    let task = tokio::spawn(async move {
        let mut write_dgrams = VecDeque::new();
        let mut sent_dgrams: VecDeque<Bytes> = VecDeque::new();
        let mut write_dgram_waiting: Option<Bytes> = None;
        let mut write_dgrams_flush_notifiers = Vec::new();
        let mut write_dgrams_ready_notifiers = Vec::new();
        let mut cwnd = 10000;
        const MAX_SEND_DGRAMS: usize = 2;
        'outer: loop {
            tokio::select! {
                res = receiver.recv() => {
                    match res {
                        Some((Request::Read { len }, respond_to)) => {
                            println!("Received a Read request: len={}", len);
                            let mut buf = BytesMut::with_capacity(len);
                            buf.resize(len - 1, 0);
                            let response = Response::Read { buf: buf.freeze() };
                            let _ = respond_to.send(response);
                        }
                        Some((Request::Write { buf }, respond_to)) => {
                            println!("Received a Write request: bytes={:?}", buf);
                            let response = Response::Write { written: buf.len() };
                            let _ = respond_to.send(response);
                        }
                        Some((Request::WriteDgramReady, respond_to)) => {
                            println!("Received a Ready request");
                            write_dgrams_ready_notifiers.push(respond_to);
                        }
                        Some((Request::WriteDgramFlush, respond_to)) => {
                            println!("Received a Flush request");
                            write_dgrams_flush_notifiers.push(respond_to);
                        }
                        None => {
                            break 'outer;
                        }
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    // Emulation of receiving QUIC packet
                    println!("Wake up");
                    if let Some(buf) = sent_dgrams.pop_front() {
                        cwnd += buf.len();
                    }
                }
            };

            if write_dgrams.len() < MAX_SEND_DGRAMS {
                if let Some(buf) = write_dgram_waiting.take() {
                    println!("Send Dgram#1: {} bytes", buf.len());
                    write_dgrams.push_back(buf);
                }
            }
            if write_dgram_waiting.is_none() {
                loop {
                    match dgram_write_receiver.try_recv() {
                        Ok(buf) => {
                            while !write_dgrams_ready_notifiers.is_empty() {
                                let respond_to = write_dgrams_ready_notifiers.pop().unwrap();
                                let response = Response::WriteDgramReady;
                                let _ = respond_to.send(response);
                            }
                            if write_dgrams.len() < MAX_SEND_DGRAMS {
                                println!("Send Dgram#2: {} bytes", buf.len());
                                write_dgrams.push_back(buf);
                            } else {
                                write_dgram_waiting = Some(buf);
                                break;
                            }
                        }
                        Err(
                            mpsc::error::TryRecvError::Empty
                            | mpsc::error::TryRecvError::Disconnected,
                        ) => {
                            while !write_dgrams_flush_notifiers.is_empty() {
                                let respond_to = write_dgrams_flush_notifiers.pop().unwrap();
                                let response = Response::WriteDgramFlush;
                                let _ = respond_to.send(response);
                            }
                            break;
                        }
                    }
                }
            }

            match stream_read_sender.try_reserve() {
                Ok(permit) => {
                    let mut buf = BytesMut::with_capacity(1300);
                    buf.resize(1300, 0);

                    permit.send(buf.freeze());
                    println!("New stream data available!");
                }
                Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    break 'outer;
                }
            }
            match dgram_read_sender.try_reserve() {
                Ok(permit) => {
                    let mut buf = BytesMut::with_capacity(1300);
                    buf.resize(1300, 0);

                    permit.send(buf.freeze());
                    println!("New datagram available!");
                }
                Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    break 'outer;
                }
            }

            // Emulation of sending QUIC packet.
            while !write_dgrams.is_empty()
                && write_dgrams.front().map(|v| v.len()).unwrap_or(0) <= cwnd
            {
                let buf = write_dgrams.pop_front().unwrap();
                cwnd -= buf.len();
                sent_dgrams.push_back(buf);
            }
            
        }
        println!("shutdown");
    });

    let mut send_stream = QuicSendStream {
        sender: PollSender::new(sender.clone()),
        stream_id: 0,
        cmd_pending: HashMap::new(),
    };

    let resp = poll_fn(|cx| Pin::new(&mut send_stream).poll_write(cx, b"1234")).await;
    println!("Received response: {:?}", resp);

    let mut recv_stream = QuicRecvStream {
        receiver: stream_read_receiver,
        local_storage: None,
        sender: PollSender::new(sender.clone()),
        stream_id: 0,
        cmd_pending: HashMap::new(),
    };
    let mut buf = [0u8; 2048];
    let mut rbuf = ReadBuf::new(&mut buf);
    rbuf.advance(100);

    let resp = poll_fn(|cx| Pin::new(&mut recv_stream).poll_read(cx, &mut rbuf)).await;
    println!("Received response: {:?}", resp);
    println!("{:?}", rbuf);

    let mut rbuf = ReadBuf::new(&mut buf);
    rbuf.advance(2048 - 652);

    let resp = poll_fn(|cx| Pin::new(&mut recv_stream).poll_read(cx, &mut rbuf)).await;
    println!("Received response: {:?}", resp);
    println!("{:?}", rbuf);

    let mut read_dgram = QuicReadDgram {
        req_sender: PollSender::new(sender.clone()),
        read_receiver: dgram_read_reciver,
    };
    let resp = poll_fn(|cx| read_dgram.poll_read_chunk(cx)).await;
    if let Ok(bytes) = resp {
        println!("Received response: bytes.len={}", bytes.len());
    }
    let resp = read_dgram.read_chunk().await;
    if let Ok(bytes) = resp {
        println!("Received response: bytes.len={}", bytes.len());
    }
    let resp = read_dgram.next().await;
    if let Some(Ok(bytes)) = resp {
        println!("Received response: bytes.len={}", bytes.len());
    } else {
        println!("resp={:?}", resp);
    }

    let mut write_dgram = QuicWriteDgram {
        cmd_sender: PollSender::new(sender),
        cmd_pending: HashMap::new(),
        write_sender: PollSender::new(dgram_write_sender),
    };

    let mut buf = BytesMut::with_capacity(1300);
    buf.resize(1300, 0);
    let buf = buf.freeze();
    let resp = poll_fn(|cx| write_dgram.poll_write_chunk(cx, &buf)).await;
    if let Ok(()) = resp {
        println!("Write Dgram: {} bytes", buf.len());
    } else {
        println!("resp={:?}", resp);
    }

    let resp = poll_fn(|cx| write_dgram.poll_write_chunk(cx, &buf)).await;
    if let Ok(()) = resp {
        println!("Write Dgram: {} bytes", buf.len());
    } else {
        println!("resp={:?}", resp);
    }

    let resp = poll_fn(|cx| write_dgram.poll_write_chunk(cx, &buf)).await;
    if let Ok(()) = resp {
        println!("Write Dgram: {} bytes", buf.len());
    } else {
        println!("resp={:?}", resp);
    }

    let resp = poll_fn(|cx| write_dgram.poll_flush(cx)).await;
    if let Ok(()) = resp {
        println!("Flush Dgram");
    } else {
        println!("resp={:?}", resp);
    }
    drop(send_stream);
    drop(recv_stream);
    drop(read_dgram);
    let _ = task.await;
}
