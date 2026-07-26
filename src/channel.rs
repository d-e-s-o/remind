use std::io::Read as _;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;

use anyhow::Context as _;
use anyhow::Result;

use postcard::from_bytes as postcard_from_bytes;
use postcard::to_io as postcard_to_io;

use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;


#[derive(Debug)]
pub struct Channel;

impl Channel {
  pub fn oneshot<T>() -> Result<(Reader<T>, Writer<T>)>
  where
    T: DeserializeOwned + Serialize,
  {
    let listener =
      TcpListener::bind(("127.0.0.1", 0)).context("failed to bind to local address")?;

    let client = Writer {
      addr: listener
        .local_addr()
        .context("failed to retrieve local address of TCP listener")?,
      _phantom: PhantomData,
    };
    let server = Reader {
      listener,
      _phantom: PhantomData,
    };

    Ok((server, client))
  }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Writer<T> {
  addr: SocketAddr,
  _phantom: PhantomData<T>,
}

impl<T> Writer<T> {
  pub fn write(self, obj: &T) -> Result<()>
  where
    T: Serialize,
  {
    let Self { addr, _phantom: _ } = self;

    let stream =
      TcpStream::connect(addr).with_context(|| format!("failed to connect to {addr}"))?;

    let _stream =
      postcard_to_io(obj, stream).context("failed to write object to oneshot channel")?;
    Ok(())
  }
}


#[derive(Debug)]
pub struct Reader<T> {
  listener: TcpListener,
  _phantom: PhantomData<T>,
}

impl<T> Reader<T> {
  pub fn read(self) -> Result<T>
  where
    T: DeserializeOwned,
  {
    let Self {
      listener,
      _phantom: _,
    } = self;

    let (mut stream, _addr) = listener
      .accept()
      .context("failed to accept onesht channel connection")?;

    let mut buf = Vec::new();
    let _cnt = stream
      .read_to_end(&mut buf)
      .context("failed to read object data")?;
    let obj =
      postcard_from_bytes::<T>(&buf).context("failed to read object from oneshot channel")?;
    Ok(obj)
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::thread;

  use postcard::from_bytes;
  use postcard::to_allocvec;


  /// Make sure that we can send and receive data over a channel.
  #[test]
  fn sending_and_receiving() {
    const OBJ: usize = 1337;

    let (reader, writer) = Channel::oneshot().unwrap();

    let sender = thread::spawn(move || {
      let () = writer.write(&OBJ).unwrap();
    });

    let obj = reader.read().unwrap();
    let () = sender.join().unwrap();

    assert_eq!(obj, OBJ);
  }

  /// Check that we can serialize and deserialize a [`Writer`].
  #[test]
  fn writer_serialization() {
    let (server, client) = Channel::oneshot::<usize>().unwrap();

    let serialized = to_allocvec(&client).unwrap();
    let deserialized = from_bytes::<Writer<usize>>(&serialized).unwrap();

    assert_eq!(client.addr, deserialized.addr);

    let () = deserialized.write(&42).unwrap();
    let obj = server.read().unwrap();
    assert_eq!(obj, 42);
  }
}
