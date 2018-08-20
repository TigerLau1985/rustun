//! STUN message transport layer.
use fibers::sync::oneshot::Link;
use futures::{Sink, Stream};
use std::net::SocketAddr;

use message::RawMessage;
use {Error, Result};

pub use self::tcp::{TcpClientTransport, TcpServerTransport};
pub use self::udp::{UdpTransport, UdpTransportBuilder};

mod tcp;
mod udp;

/// The type of `SinkItem` of [MessageSink](trait.MessageSink.html).
///
/// The first element of the tuple is the address of a destination peer.
/// The second is the sending message.
/// The third is the link with the sending transaction (if it is not `None`);
/// If it is terminated, you can receive the notification from the link.
/// And if it is a request transaction,
/// you can terminate it (e.g., retransmissions in UDP) by dropping own link.
pub type MessageSinkItem = (SocketAddr, RawMessage, Option<Link<(), Error, (), ()>>);

/// A marker trait representing that the implementation can be used as
/// the sending side of message transport layer.
pub trait MessageSink: Sink<SinkItem = MessageSinkItem, SinkError = Error> {}

/// A marker trait representing that the implementation can be used as
/// the receiving side of message transport layer.
pub trait MessageStream: Stream<Item = (SocketAddr, Result<RawMessage>), Error = Error> {}

/// A marker trait representing that the implementation can be used as message transport layer.
pub trait Transport: MessageSink + MessageStream {}
