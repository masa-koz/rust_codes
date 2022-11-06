use std::boxed::Box;
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::future::Future;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::future::{poll_fn, FutureExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{PollSendError, PollSender};

struct QuicRecvStream {
    sender: PollSender<(Request, oneshot::Sender<Response>)>,
    stream_id: u64,
    req_pending: HashMap<usize, RequestState>,
}

impl<'a> AsyncRead for QuicRecvStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();

        let key: usize = std::ptr::addr_of!(cx) as usize;
        if !me.req_pending.contains_key(&key) {
            let msg = Request::Read { len: buf.remaining() };
            me.req_pending.insert(
                key,
                RequestState::State0 {
                    msg: Some(msg),
                },
            );
        }
    
        let res = match me.poll_generic(key, cx) {
            Poll::Ready(Ok(Response::Read { buf: bytes })) => {
                buf.put_slice(&bytes[..]);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(_)) => {
                panic!("Invalid response!");
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "oneshot recv failed")))
            }
            Poll::Pending => {
                Poll::Pending
            },
        };
        if !res.is_pending() {
            me.req_pending.remove(&key);
        }
        res
    }
}

impl QuicRecvStream {
    fn poll_generic(
        &mut self,
        key: usize, 
        cx: &mut Context,
    ) -> Poll<std::result::Result<Response, RequestError>> {
        loop {
            match self.req_pending.get_mut(&key).unwrap() {
                RequestState::State0 {
                    ref mut msg,
                } => {
                    println!("State0");
                    match self.sender.poll_reserve(cx) {
                        Poll::Ready(Ok(())) => {
                            let msg = msg.take().unwrap();
                            let (send, mut recv) = oneshot::channel();
                            if let Err(e) = self.sender.send_item((msg, send)) {
                                return Poll::Ready(Err(RequestError::SendError(e)));    
                            }  
                            *self.req_pending.get_mut(&key).unwrap() = RequestState::State1 {
                                recv: Some(recv),
                            };        
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(RequestError::SendError(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }

                    }
                }
                RequestState::State1 { ref mut recv } => {
                    println!("State1");
                    match recv.as_mut().unwrap().poll_unpin(cx) {
                        Poll::Ready(Ok(response)) => {
                            *self.req_pending.get_mut(&key).unwrap() = RequestState::Terminated;
                            return Poll::Ready(Ok(response));
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(RequestError::RecvError(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                RequestState::Terminated => {
                    panic!("future polled after completion");
                }
            }
        }
    }
}

struct QuicSendStream {
    sender: PollSender<(Request, oneshot::Sender<Response>)>,
    stream_id: u64,
    req_pending: HashMap<usize, RequestState>,
}

impl<'a> AsyncWrite for QuicSendStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let me = self.get_mut();

        let key: usize = std::ptr::addr_of!(cx) as usize;
        if !me.req_pending.contains_key(&key) {
            let mut bytes_mut = BytesMut::new();
            bytes_mut.extend_from_slice(buf);
            let msg = Request::Write { buf: bytes_mut.freeze() };
            me.req_pending.insert(
                key,
                RequestState::State0 {
                    msg: Some(msg),
                },
            );
        }
    
        let res = match me.poll_generic(key, cx) {
            Poll::Ready(Ok(Response::Write { written })) => {
                Poll::Ready(Ok(written))
            }
            Poll::Ready(Ok(_)) => {
                panic!("Invalid response!");
            }
            Poll::Ready(Err(e)) => {
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "oneshot recv failed")))
            }
            Poll::Pending => {
                Poll::Pending
            },
        };
        if !res.is_pending() {
            me.req_pending.remove(&key);
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

impl QuicSendStream {
    fn poll_generic(
        &mut self,
        key: usize, 
        cx: &mut Context,
    ) -> Poll<std::result::Result<Response, RequestError>> {
        loop {
            match self.req_pending.get_mut(&key).unwrap() {
                RequestState::State0 {
                    ref mut msg,
                } => {
                    println!("State0");
                    match self.sender.poll_reserve(cx) {
                        Poll::Ready(Ok(())) => {
                            let msg = msg.take().unwrap();
                            let (send, mut recv) = oneshot::channel();
                            if let Err(e) = self.sender.send_item((msg, send)) {
                                return Poll::Ready(Err(RequestError::SendError(e)));    
                            }  
                            *self.req_pending.get_mut(&key).unwrap() = RequestState::State1 {
                                recv: Some(recv),
                            };        
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(RequestError::SendError(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }

                    }
                }
                RequestState::State1 { ref mut recv } => {
                    println!("State1");
                    match recv.as_mut().unwrap().poll_unpin(cx) {
                        Poll::Ready(Ok(response)) => {
                            *self.req_pending.get_mut(&key).unwrap() = RequestState::Terminated;
                            return Poll::Ready(Ok(response));
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(RequestError::RecvError(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                RequestState::Terminated => {
                    panic!("future polled after completion");
                }
            }
        }
    }
}

struct QuicReadDgram {
    receiver: mpsc::Receiver<Response>,
}

impl QuicReadDgram {
    async fn read_chunk(&mut self) -> std::result::Result<Bytes, ReadDgramError> {
        ReadChunk {
            read_dgram: self,
        }.await
    }

    fn poll_read_chunk(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<Bytes, ReadDgramError>> {
        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some(Response::ReadDgram { bytes })) => {
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
enum ReadDgramError {
    ConnectionClosed,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Request {
    Read { len: usize },
    Write { buf: Bytes },
}

#[derive(Debug)]
enum Response {
    Read { buf: Bytes },
    Write { written: usize },
    ReadDgram { bytes: Bytes },
}

#[derive(Debug)]
enum RequestError {
    SendError(PollSendError<(Request, oneshot::Sender<Response>)>),
    RecvError(oneshot::error::RecvError),
}

enum RequestState {
    State0 {
        msg: Option<Request>,
    },
    State1 {
        recv: Option<oneshot::Receiver<Response>>,
    },
    Terminated,
}

#[tokio::main]
async fn main() {
    let (sender, mut receiver) = mpsc::channel::<(Request, oneshot::Sender<Response>)>(128);
    let (dgram_writer, dgram_reader) = mpsc::channel(128);

    let task = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Some((Request::Read {len }, respond_to)) => {
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
                None => {
                    break;
                }
            }
        }
    });

    let task1 = tokio::spawn(async move {
        for _ in 0..2 {
            let mut buf = BytesMut::with_capacity(1300);
            buf.resize(1300, 0);
            let resp = Response::ReadDgram { bytes: buf.freeze() };
            let _ = dgram_writer.send(resp).await;
        }
    });

    let mut send_stream = QuicSendStream {
        sender: PollSender::new(sender.clone()),
        stream_id: 0,
        req_pending: HashMap::new(),
    };

    let resp = poll_fn(|cx| {
        Pin::new(&mut send_stream).poll_write(cx, b"1234")
    })
    .await;
    println!("Received response: {:?}", resp);

    let mut recv_stream = QuicRecvStream {
        sender: PollSender::new(sender),
        stream_id: 0,
        req_pending: HashMap::new(),
    };
    let mut buf = [0u8; 2048];
    let mut buf = ReadBuf::new(&mut buf);
    buf.advance(100);

    let resp = poll_fn(|cx| {
        Pin::new(&mut recv_stream).poll_read(cx, &mut buf)
    })
    .await;
    println!("Received response: {:?}", resp);
    println!("{:?}", buf);

    let mut read_dgram = QuicReadDgram {
        receiver: dgram_reader,
    };
    let resp = poll_fn(|cx| {
        read_dgram.poll_read_chunk(cx)
    }).await;
    println!("Received response: {:?}", resp);
    let resp = read_dgram.read_chunk().await;
    println!("Received response: {:?}", resp);

    drop(send_stream);
    drop(recv_stream);
    drop(read_dgram);
    task.await;
    task1.await;
}
