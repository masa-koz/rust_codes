use std::boxed::Box;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use futures::future::{poll_fn, FutureExt};
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::{PollSendError, PollSender};

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
        let key: usize = std::ptr::addr_of!(cx) as usize;
        println!("key: {}", key);

        let me = self.get_mut();

        if !me.req_pending.contains_key(&key) {
            let msg = Request::Write { buf: Bytes::new() };
            me.req_pending.insert(
                key,
                RequestState::State0 {
                    msg: Some(msg),
                },
            );
        }
    
        match me.poll_generic(key, cx) {
            Poll::Ready(Ok(Response::Write { written })) => {
                return Poll::Ready(Ok(written));
            }
            Poll::Ready(Ok(_)) => {
                panic!("Invalid response!");
            }
            Poll::Ready(Err(e)) => {
                return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, "oneshot recv failed")));
            }
            Poll::Pending => {
                return Poll::Pending;
            },
        }
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

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Request {
    Read,
    Write { buf: Bytes },
}

#[derive(Debug)]
enum Response {
    Read { buf: Bytes },
    Write { written: usize },
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
    let (sender, mut receiver) = mpsc::channel::<(Request, oneshot::Sender<Response>)>(1);

    let task = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Some((Request::Read, respond_to)) => {
                    let buf = BytesMut::with_capacity(2048);
                    let response = Response::Read { buf: buf.freeze() };
                    let _ = respond_to.send(response);
                }
                Some((Request::Write { buf }, respond_to)) => {
                    let response = Response::Write { written: buf.len() };
                    let _ = respond_to.send(response);
                }
                None => {}
            }
            break;
        }
    });

    let mut send_stream = QuicSendStream {
        sender: PollSender::new(sender),
        stream_id: 0,
        req_pending: HashMap::new(),
    };

    let resp = poll_fn(move |cx| {
        //fut.poll_unpin(cx)
        Pin::new(&mut send_stream).poll_write(cx, b"")
    })
    .await;
    println!("Received response: {:?}", resp);
    task.await;
}
