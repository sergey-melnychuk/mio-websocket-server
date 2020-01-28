// Benchmarks:
// $ ab -n 1000000 -c 128 -k http://127.0.0.1:9000/
// $ wrk -d 30s -t 4 -c 128 http://127.0.0.1:9000/

mod pool;

use mio::net::{TcpListener, TcpStream};
use mio::{Poll, Token, Ready, PollOpt, Events};
use std::collections::HashMap;
use std::io::{Read, Write};

use crate::pool::ThreadPool;
use std::time::Duration;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};

extern crate log;
extern crate env_logger;
use log::debug;

static RESPONSE: &str = "HTTP/1.1 200 OK
Content-Type: text/html
Connection: keep-alive
Content-Length: 6

hello
";

fn is_double_crnl(window: &[u8]) -> bool {
    window.len() >= 4 &&
        (window[0] == b'\r') &&
        (window[1] == b'\n') &&
        (window[2] == b'\r') &&
        (window[3] == b'\n')
    // this is much slower (60k vs 75k rps):
    // window == [b'\r', b'\n', b'\r', b'\n']
}

fn blocks(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::WouldBlock
}

fn skip<T>(n: usize, vec: Vec<T>) -> Vec<T> {
    vec.into_iter().skip(n).collect()
}

#[derive(Debug)]
struct Handler {
    token: Token,
    socket: TcpStream,
    recv_buffer: Vec<u8>,
    send_buffer: Vec<u8>,
    is_open: bool,
    do_recv: bool,
    do_send: bool,
}

impl Handler {
    fn init(token: Token, socket: TcpStream) -> Handler {
        Handler {
            token,
            socket,
            recv_buffer: Vec::with_capacity(1024),
            send_buffer: Vec::with_capacity(1024),
            is_open: true,
            do_recv: true,
            do_send: true,
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
                Ok(n) =>
                    self.recv_buffer.extend_from_slice(&buffer[0..n]),
                Err(ref e) if blocks(e) =>
                    break,
                Err(_) =>
                    break
            }
        }
    }

    fn push(&mut self) {
        debug!("token {} push", self.token.0);
        match self.socket.write_all(&self.send_buffer[..]) {
            Ok(_) => (),
            Err(_) => {
                self.is_open = false;
                return;
            }
        }
        self.send_buffer.clear();
    }

    fn get<T>(&mut self, f: fn(&Vec<u8>) -> (usize, Option<T>)) -> Option<T> {
        let (consumed, result_opt) = f(&self.recv_buffer);
        debug!("token {} get ({} bytes)", self.token.0, consumed);

        if consumed > 0 {
            let remaining = skip(consumed, self.recv_buffer.to_owned());
            self.recv_buffer = remaining;
        }

        result_opt
    }

    fn put<T>(&mut self, result: T, f: fn(T) -> Vec<u8>) {
        debug!("token {} put", self.token.0);
        let mut bytes = f(result);
        self.send_buffer.append(&mut bytes);
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

                if handler.do_recv {
                    handler.pull();
                    let opt = handler.get(|bytes| {
                        let found = bytes
                            .windows(4)
                            .any(|window| is_double_crnl(window));

                        if found {
                            (bytes.len(), Some(true))
                        } else {
                            (0, None)
                        }
                    });
                    if opt.unwrap_or(false) {
                        handler.put(RESPONSE, |r: &str| r.as_bytes().to_owned().to_vec());
                    }
                }

                if handler.do_send {
                    handler.push();
                }

                ready_tx.send(handler).unwrap();
            }
        });
    }

    let mut events = Events::with_capacity(1024);
    loop {
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
                                              Ready::readable(), PollOpt::edge())
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
                    match handlers.remove(&token) {
                        Some(mut handler) => {
                            handler.do_recv = true;
                            tx.send(handler).unwrap();
                        },
                        _ => ()
                    }
                },
                token if event.readiness().is_writable() => {
                    debug!("token {} writable", token.0);
                    match handlers.remove(&token) {
                        Some(mut handler) => {
                            handler.do_send = true;
                            tx.send(handler).unwrap();
                        },
                        _ => ()
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
                    if handler.send_buffer.len() > 0 {
                        debug!("token {} has something to send", handler.token.0);
                        poll.reregister(&handler.socket, handler.token,
                                        Ready::writable(), PollOpt::edge() | PollOpt::oneshot())
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
