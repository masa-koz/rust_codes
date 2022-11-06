use std::boxed::Box;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;

use futures::future::{poll_fn, FutureExt};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
enum Message {
    Read { respond_to: oneshot::Sender<i32> },
}

struct RecvStream<'a> {
    req_fut: Option<
        Pin<
            Box<dyn Future<Output = Result<(), tokio::sync::mpsc::error::SendError<Message>>> + 'a>,
        >,
    >,
}

enum ReadState<'a> {
    State0 {
        sender: &'a mpsc::Sender<Message>,
        msg: Option<Message>,
        recv: Option<&'a mut oneshot::Receiver<i32>>,
    },
    State1 {
        fut: Pin<Box<dyn Future<Output = Result<(), tokio::sync::mpsc::error::SendError<Message>>> + 'a>>,
        recv: Option<&'a mut oneshot::Receiver<i32>>,
    },
    State2 {
        recv: Option<&'a mut oneshot::Receiver<i32>>,
    },
    Terminated,
}

impl<'a> Future for ReadState<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        loop {
            match *self {
                ReadState::State0 { sender, ref mut msg, ref mut recv} => {
                    println!("State0");
                    let msg = msg.take().unwrap();
                    let recv = recv.take().unwrap();
                    *self = ReadState::State1 { fut: Box::pin(sender.send(msg)), recv: Some(recv) };
                }
                ReadState::State1 { ref mut fut, ref mut recv } => {
                    println!("State1");
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(_) => {
                            let recv = recv.take().unwrap();
                            *self = ReadState::State2 { recv: Some(recv)};
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                ReadState::State2 { ref mut recv } => {
                    println!("State2");
                    match recv.as_mut().unwrap().poll_unpin(cx) {
                        Poll::Ready(a) => {
                            println!("{:?}", a);
                            *self = ReadState::Terminated;
                            return Poll::Ready(());
                        }
                        Poll::Pending => {
                            return Poll::Pending;
                        }
                    }
                }
                ReadState::Terminated => {
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
                Some(Message::Read { respond_to }) => {
                    println!("Received request");
                    let _ = respond_to.send(1);
                }
                None => {
                }
            }
            break;
        }
    });

    let (send, mut recv) = oneshot::channel();
    let msg = Message::Read { respond_to: send };
    let mut fut = ReadState::State0 { sender: &sender, msg: Some(msg), recv: Some(&mut recv) };

    let _ = poll_fn(move |cx| {
        fut.poll_unpin(cx)
        /*        
        let mut recv_stream = RecvStream { req_fut: None };
        loop {
            recv_stream.req_fut = Some(Box::pin(sender.send(msg)));
            let res = if let Some(mut fut) = recv_stream.req_fut {
                fut.as_mut().poll(cx)
            } else {
                Poll::Ready(Ok(()))
            };
        }
        */
        //let resp = recv.await.unwrap();
        //println!("Received response: {}", resp);
    })
    .await;
    task.await;
}
