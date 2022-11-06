use std::boxed::Box;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use std::collections::HashMap;

use bytes::{Bytes, BytesMut};
use futures::future::{poll_fn, FutureExt};
use tokio::sync::{mpsc, oneshot};
use tokio::io::AsyncWrite;

struct QuicSendStream<'a> {
    sender: mpsc::Sender<Request>,
    stream_id: u64,
    req_pending: HashMap<usize, RequestState<'a>>,
}

impl<'a> AsyncWrite for QuicSendStream<'a> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Request {
    Read { respond_to: oneshot::Sender<Response> },
    Write { buf: Bytes, respond_to: oneshot::Sender<Response>}
}

#[derive(Debug)]
enum Response {
    Read { buf: Bytes },
    Write { written: usize },
}

#[derive(Debug)]
enum RequestError {
    SendError(mpsc::error::SendError<Request>),
    RecvError(oneshot::error::RecvError),
}

enum RequestState<'a> {
    State0 {
        sender: &'a mpsc::Sender<Request>,
        msg: Option<Request>,
        recv: Option<&'a mut oneshot::Receiver<Response>>,
    },
    State1 {
        fut: Pin<Box<dyn Future<Output = std::result::Result<(), tokio::sync::mpsc::error::SendError<Request>>> + 'a>>,
        recv: Option<&'a mut oneshot::Receiver<Response>>,
    },
    State2 {
        recv: Option<&'a mut oneshot::Receiver<Response>>,
    },
    Terminated,
}

impl<'a> Future for RequestState<'a> {
    type Output = std::result::Result<Response, RequestError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<std::result::Result<Response, RequestError>> {
        loop {
            match *self {
                RequestState::State0 { sender, ref mut msg, ref mut recv} => {
                    println!("State0");
                    let msg = msg.take().unwrap();
                    let recv = recv.take().unwrap();
                    *self = RequestState::State1 { fut: Box::pin(sender.send(msg)), recv: Some(recv) };
                }
                RequestState::State1 { ref mut fut, ref mut recv } => {
                    println!("State1");
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(())) => {
                            let recv = recv.take().unwrap();
                            *self = RequestState::State2 { recv: Some(recv)};
                        }
                        Poll::Ready(Err(e)) => {
                            return Poll::Ready(Err(RequestError::SendError(e)));
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                RequestState::State2 { ref mut recv } => {
                    println!("State2");
                    match recv.as_mut().unwrap().poll_unpin(cx) {
                        Poll::Ready(Ok(response)) => {
                            *self = RequestState::Terminated;
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
#[tokio::main]
async fn main() {
    let (sender, mut receiver) = mpsc::channel(1);

    let task = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Some(Request::Read { respond_to }) => {
                    let buf = BytesMut::with_capacity(2048);
                    let response = Response::Read {buf: buf.freeze()};
                    let _ = respond_to.send(response);
                }
                Some(Request::Write { buf, respond_to }) => {
                    let response = Response::Write { written:buf.len() };
                    let _ = respond_to.send(response);
                }
                None => {
                }
            }
            break;
        }
    });

    let (send, mut recv) = oneshot::channel();
    let msg = Request::Read { respond_to: send };
    let mut fut = RequestState::State0 { sender: &sender, msg: Some(msg), recv: Some(&mut recv) };

    let resp = poll_fn(move |cx| {
        fut.poll_unpin(cx)
    })
    .await;
    println!("Received response: {:?}", resp);
    task.await;
}
