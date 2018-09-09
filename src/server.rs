use bytecodec::marker::Never;
use factory::Factory;
use fibers::net::futures::{TcpListenerBind, UdpSocketBind};
use fibers::net::streams::Incoming;
use fibers::net::{TcpListener, UdpSocket};
use fibers::sync::mpsc;
use fibers::{BoxSpawn, Spawn};
use futures::future::Either;
use futures::{self, Async, Future, Poll, Stream};
use std::fmt;
use std::net::SocketAddr;
use stun_codec::Attribute;

use channel::{Channel, RecvMessage};
use message::{Indication, InvalidMessage, Request, Response};
use transport::{
    RetransmitTransporter, StunTransport, StunUdpTransporter, TcpTransporter, UdpTransporter,
};
use {Error, ErrorKind};

#[derive(Debug)]
pub struct UdpServer<H: HandleMessage>(UdpServerInner<H>);
impl<H: HandleMessage> UdpServer<H> {
    pub fn start<S>(spawner: S, bind_addr: SocketAddr, handler: H) -> Self
    where
        S: Spawn + Send + 'static,
    {
        UdpServer(UdpServerInner::Binding {
            future: UdpSocket::bind(bind_addr),
            spawner: Some(spawner.boxed()),
            handler: Some(handler),
        })
    }
}
impl<H: HandleMessage> Future for UdpServer<H> {
    type Item = Never;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

enum UdpServerInner<H: HandleMessage> {
    Binding {
        future: UdpSocketBind,
        spawner: Option<BoxSpawn>,
        handler: Option<H>,
    },
    Running {
        driver: HandlerDriver<H, StunUdpTransporter<H::Attribute>>,
    },
}
impl<H: HandleMessage> Future for UdpServerInner<H> {
    type Item = Never;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self {
                UdpServerInner::Binding {
                    future,
                    spawner,
                    handler,
                } => {
                    if let Async::Ready(socket) = track!(future.poll().map_err(Error::from))? {
                        let transporter = RetransmitTransporter::new(UdpTransporter::from(socket));
                        let channel = Channel::new(transporter);
                        let driver = HandlerDriver::new(
                            spawner.take().expect("never fails"),
                            handler.take().expect("never fails"),
                            channel,
                        );
                        UdpServerInner::Running { driver }
                    } else {
                        break;
                    }
                }
                UdpServerInner::Running { driver } => {
                    if let Async::Ready(()) = track!(driver.poll())? {
                        track_panic!(ErrorKind::Other, "UDP server unexpectedly terminated");
                    } else {
                        break;
                    }
                }
            };
            *self = next;
        }
        Ok(Async::NotReady)
    }
}
impl<H: HandleMessage> fmt::Debug for UdpServerInner<H> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UdpServerInner::Binding { .. } => write!(f, "Binding {{ .. }}"),
            UdpServerInner::Running { .. } => write!(f, "Running {{ .. }}"),
        }
    }
}

#[derive(Debug)]
pub struct TcpServer<S, H>(TcpServerInner<S, H>);
impl<S, H> TcpServer<S, H>
where
    S: Spawn + Clone + Send + 'static,
    H: Factory,
    H::Item: HandleMessage,
{
    pub fn start(spawner: S, bind_addr: SocketAddr, handler_factory: H) -> Self {
        let inner = TcpServerInner::Binding {
            future: TcpListener::bind(bind_addr),
            spawner: Some(spawner),
            handler_factory: Some(handler_factory),
        };
        TcpServer(inner)
    }
}
impl<S, H> Future for TcpServer<S, H>
where
    S: Spawn + Clone + Send + 'static,
    H: Factory,
    H::Item: HandleMessage + Send + 'static,
    <<H::Item as HandleMessage>::Attribute as Attribute>::Decoder: Send + 'static,
    <<H::Item as HandleMessage>::Attribute as Attribute>::Encoder: Send + 'static,
{
    type Item = Never;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

enum TcpServerInner<S, H> {
    Binding {
        future: TcpListenerBind,
        spawner: Option<S>,
        handler_factory: Option<H>,
    },
    Listening {
        incoming: Incoming,
        spawner: S,
        handler_factory: H,
    },
}
impl<S, H> Future for TcpServerInner<S, H>
where
    S: Spawn + Clone + Send + 'static,
    H: Factory,
    H::Item: HandleMessage + Send + 'static,
    <<H::Item as HandleMessage>::Attribute as Attribute>::Decoder: Send + 'static,
    <<H::Item as HandleMessage>::Attribute as Attribute>::Encoder: Send + 'static,
{
    type Item = Never;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let next = match self {
                TcpServerInner::Binding {
                    future,
                    spawner,
                    handler_factory,
                } => {
                    if let Async::Ready(listener) = track!(future.poll().map_err(Error::from))? {
                        TcpServerInner::Listening {
                            incoming: listener.incoming(),
                            spawner: spawner.take().expect("never fails"),
                            handler_factory: handler_factory.take().expect("never fails"),
                        }
                    } else {
                        break;
                    }
                }
                TcpServerInner::Listening {
                    incoming,
                    spawner,
                    handler_factory,
                } => {
                    if let Async::Ready(client) = track!(incoming.poll().map_err(Error::from))? {
                        if let Some((future, addr)) = client {
                            let boxed_spawner = spawner.clone().boxed();
                            let mut handler = handler_factory.create();
                            let future = future.then(move |result| match result {
                                Err(e) => {
                                    let e = track!(Error::from(e));
                                    handler.handle_transport_error(&e);
                                    Either::A(futures::failed(e))
                                }
                                Ok(stream) => {
                                    let transporter = TcpTransporter::from((addr, stream));
                                    let channel = Channel::new(transporter);
                                    Either::B(HandlerDriver::new(boxed_spawner, handler, channel))
                                }
                            });
                            spawner.spawn(future.map_err(|_| ()));
                        } else {
                            track_panic!(ErrorKind::Other, "TCP server unexpectedly terminated");
                        }
                    }
                    break;
                }
            };
            *self = next;
        }
        Ok(Async::NotReady)
    }
}
impl<S, H> fmt::Debug for TcpServerInner<S, H> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TcpServerInner::Binding { .. } => write!(f, "Binding {{ .. }}"),
            TcpServerInner::Listening { .. } => write!(f, "Listening {{ .. }}"),
        }
    }
}

pub enum Action<T> {
    Reply(T),
    FutureReply(Box<Future<Item = T, Error = Never> + Send + 'static>),
    NoReply,
    FutureNoReply(Box<Future<Item = (), Error = Never> + Send + 'static>),
}
impl<T: fmt::Debug> fmt::Debug for Action<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Action::Reply(t) => write!(f, "Reply({:?})", t),
            Action::FutureReply(_) => write!(f, "FutureReply(_)"),
            Action::NoReply => write!(f, "NoReply"),
            Action::FutureNoReply(_) => write!(f, "FutureNoReply(_)"),
        }
    }
}

#[allow(unused_variables)]
pub trait HandleMessage {
    type Attribute: Attribute + Send + 'static;

    fn handle_call(
        &mut self,
        peer: SocketAddr,
        request: Request<Self::Attribute>,
    ) -> Action<Response<Self::Attribute>> {
        Action::NoReply
    }

    fn handle_cast(
        &mut self,
        peer: SocketAddr,
        indication: Indication<Self::Attribute>,
    ) -> Action<Never> {
        Action::NoReply
    }

    fn handle_invalid_message(
        &mut self,
        peer: SocketAddr,
        message: InvalidMessage,
    ) -> Action<Response<Self::Attribute>> {
        Action::NoReply
    }

    fn handle_transport_error(&mut self, error: &Error) {}
}

#[derive(Debug)]
struct HandlerDriver<H: HandleMessage, T> {
    spawner: BoxSpawn,
    handler: H,
    channel: Channel<H::Attribute, T>,
    response_tx: mpsc::Sender<(SocketAddr, Response<H::Attribute>)>,
    response_rx: mpsc::Receiver<(SocketAddr, Response<H::Attribute>)>,
}
impl<H, T> HandlerDriver<H, T>
where
    H: HandleMessage,
    T: StunTransport<H::Attribute>,
{
    fn new(spawner: BoxSpawn, handler: H, channel: Channel<H::Attribute, T>) -> Self {
        let (response_tx, response_rx) = mpsc::channel();
        HandlerDriver {
            spawner,
            handler,
            channel,
            response_tx,
            response_rx,
        }
    }

    fn handle_message(&mut self, peer: SocketAddr, message: RecvMessage<H::Attribute>) {
        match message {
            RecvMessage::Indication(m) => self.handle_indication(peer, m),
            RecvMessage::Request(m) => self.handle_request(peer, m),
            RecvMessage::Invalid(m) => self.handle_invalid_message(peer, m),
        }
    }

    fn handle_indication(&mut self, peer: SocketAddr, indication: Indication<H::Attribute>) {
        match self.handler.handle_cast(peer, indication) {
            Action::NoReply => {}
            Action::FutureNoReply(future) => self.spawner.spawn(future.map_err(|_| unreachable!())),
            _ => unreachable!(),
        }
    }

    fn handle_request(&mut self, peer: SocketAddr, request: Request<H::Attribute>) {
        match self.handler.handle_call(peer, request) {
            Action::NoReply => {}
            Action::FutureNoReply(future) => self.spawner.spawn(future.map_err(|_| unreachable!())),
            Action::Reply(m) => self.channel.reply(peer, m),
            Action::FutureReply(future) => {
                let tx = self.response_tx.clone();
                self.spawner.spawn(
                    future
                        .map(move |response| {
                            let _ = tx.send((peer, response));
                            ()
                        })
                        .map_err(|_| unreachable!()),
                );
            }
        }
    }

    fn handle_invalid_message(&mut self, peer: SocketAddr, message: InvalidMessage) {
        match self.handler.handle_invalid_message(peer, message) {
            Action::NoReply => {}
            Action::FutureNoReply(future) => self.spawner.spawn(future.map_err(|_| unreachable!())),
            Action::Reply(m) => self.channel.reply(peer, m),
            Action::FutureReply(future) => {
                let tx = self.response_tx.clone();
                self.spawner.spawn(
                    future
                        .map(move |response| {
                            let _ = tx.send((peer, response));
                            ()
                        })
                        .map_err(|_| unreachable!()),
                );
            }
        }
    }
}
impl<H, T> Future for HandlerDriver<H, T>
where
    H: HandleMessage,
    T: StunTransport<H::Attribute>,
{
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut did_something = true;
        while did_something {
            did_something = false;

            match track!(self.channel.poll()) {
                Err(e) => {
                    self.handler.handle_transport_error(&e);
                    return Err(e);
                }
                Ok(Async::NotReady) => {}
                Ok(Async::Ready(message)) => {
                    if let Some((peer, message)) = message {
                        self.handle_message(peer, message);
                    } else {
                        return Ok(Async::Ready(()));
                    }
                    did_something = true;
                }
            }
            if let Async::Ready(item) = self.response_rx.poll().expect("never fails") {
                let (peer, response) = item.expect("never fails");
                self.channel.reply(peer, response);
                did_something = true;
            }
        }
        Ok(Async::NotReady)
    }
}