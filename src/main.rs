mod pool;

use mio::net::{TcpListener, TcpStream};
use mio::{Poll, Token, Ready, PollOpt, Events};
use std::collections::HashMap;
use std::io::{Read, Write};

use crate::pool::ThreadPool;
use std::time::Duration;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};

use parser_combinators::stream::ByteStream;
use parser_combinators::http::parse_http_request;

use log::debug;
extern crate log;
extern crate env_logger;

static RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: keep-alive\r\nContent-Length: 6\r\n\r\nhello\n";

fn blocks(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::WouldBlock
}

#[derive(Debug)]
struct Handler {
    token: Token,
    socket: TcpStream,
    is_open: bool,
    recv_stream: ByteStream,
    send_stream: ByteStream,
}

impl Handler {
    fn init(token: Token, socket: TcpStream) -> Handler {
        Handler {
            token,
            socket,
            is_open: true,
            recv_stream: ByteStream::with_capacity(1024),
            send_stream: ByteStream::with_capacity(1024),
        }
    }

    fn pull(&mut self) {
        debug!("token {} pull", self.token.0);
        let mut buffer = [0 as u8; 1024];
        loop {
            let read = self.socket.read(&mut buffer);
            match read {
                Ok(0) => {
                    self.is_open = false;
                    return
                },
                Ok(n) => {
                    self.recv_stream.put(&buffer[0..n]);
                },
                Err(ref e) if blocks(e) =>
                    break,
                Err(_) =>
                    break
            }
        }
    }

    fn push(&mut self) {
        debug!("token {} push", self.token.0);
        match self.socket.write_all(self.send_stream.as_ref()) {
            Ok(_) => (),
            Err(_) => {
                self.is_open = false;
                return;
            }
        }
        self.send_stream.clear();
    }

    fn put<T>(&mut self, result: T, f: fn(T) -> Vec<u8>) {
        debug!("token {} put", self.token.0);
        let bytes = f(result);
        self.send_stream.put(&bytes);
    }
}

fn main() {
    env_logger::init();

    let address = "0.0.0.0:9000";
    let listener = TcpListener::bind(&address.parse().unwrap()).unwrap();

    let poll = Poll::new().unwrap();
    poll.register(
        &listener,
        Token(0),
        Ready::readable(),
        PollOpt::edge()).unwrap();

    let mut counter: usize = 0;
    let mut handlers: HashMap<Token, Handler> = HashMap::new();

    let (tx, rx): (Sender<Handler>, Receiver<Handler>) = channel();
    let rx = Arc::new(Mutex::new(rx));

    let (ready_tx, ready_rx): (Sender<Handler>, Receiver<Handler>) = channel();

    let mut pool = ThreadPool::new(4);
    for _ in 0..pool.size() {
        let rx = Arc::clone(&rx);
        let ready_tx = ready_tx.clone();
        pool.submit(move || {
            loop {
                let mut handler = rx.lock().unwrap().recv().unwrap();
                debug!("token {} background thread", handler.token.0);

                handler.pull();

                let req_opt = parse_http_request(&mut handler.recv_stream);
                if let Some(req) = req_opt {
                    debug!("request: {:?}", req);
                    handler.recv_stream.pull();
                    handler.put(RESPONSE, |r: &str| r.as_bytes().to_owned().to_vec());
                };

                if handler.send_stream.len() > 0 {
                    handler.push();
                }

                ready_tx.send(handler).unwrap();
            }
        });
    }

    let mut events = Events::with_capacity(1024);
    loop {
        // FIXME All networking code still happens in single thread!
        poll.poll(&mut events, Some(Duration::from_millis(20))).unwrap();
        for event in &events {
            match event.token() {
                Token(0) => {
                    loop {
                        match listener.accept() {
                            Ok((socket, _)) => {
                                counter += 1;
                                let token = Token(counter);
                                poll.register(&socket, token,
                                              Ready::readable(),
                                              PollOpt::edge())
                                    .unwrap();
                                handlers.insert(token, Handler::init(token, socket));
                                debug!("token {} connected", token.0);
                            },
                            Err(_) => break
                        }
                    }
                },
                token if event.readiness().is_readable() => {
                    debug!("token {} readable", token.0);
                    if let Some(handler) = handlers.remove(&token) {
                        tx.send(handler).unwrap();
                    }
                },
                token if event.readiness().is_writable() => {
                    debug!("token {} writable", token.0);
                    if let Some(handler) = handlers.remove(&token) {
                        tx.send(handler).unwrap();
                    }
                },
                _ => unreachable!()
            }
        }

        loop {
            let opt = ready_rx.try_recv();
            match opt {
                Ok(handler) if !handler.is_open => {
                    debug!("token {} closed", handler.token.0);
                },
                Ok(handler) => {
                    if handler.send_stream.len() > 0 {
                        debug!("token {} has something to send", handler.token.0);
                        poll.reregister(&handler.socket, handler.token,
                                        Ready::writable(),
                                        PollOpt::edge() | PollOpt::oneshot())
                            .unwrap();
                    } else {
                        debug!("token {} can receive something", handler.token.0);
                        poll.reregister(&handler.socket, handler.token,
                                        Ready::readable(), PollOpt::edge())
                            .unwrap();
                    }
                    handlers.insert(handler.token, handler);
                },
                _ => break,
            }
        }
    }
}
