// Benchmarks:
// $ ab -n 1000000 -c 128 -k http://127.0.0.1:9000/
// $ wrk -d 30s -t 4 -c 128 http://127.0.0.1:9000/

use mio::net::{TcpListener, TcpStream};
use mio::{Poll, Token, Ready, PollOpt, Events};
use std::collections::HashMap;
use std::io::{Read, Write};

static RESPONSE: &str = "HTTP/1.1 200 OK
Content-Type: text/html
Connection: keep-alive
Content-Length: 6

hello
";

fn is_double_crnl(window: &[u8]) -> bool {
    window.len() >= 4 &&
        (window[0] == '\r' as u8) &&
        (window[1] == '\n' as u8) &&
        (window[2] == '\r' as u8) &&
        (window[3] == '\n' as u8)
}

fn blocks(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::WouldBlock
}

fn skip<T>(n: usize, vec: Vec<T>) -> Vec<T> {
    vec.into_iter().skip(n).collect()
}

struct Handler<'a> {
    poll: &'a Poll,
    token: Token,
    socket: TcpStream,
    recv_buffer: Vec<u8>,
    send_buffer: Vec<u8>,
    is_open: bool,
}

impl Handler<'_> {
    fn init(poll: &Poll, token: Token, socket: TcpStream) -> Handler {
        poll.register(&socket, token, Ready::readable(), PollOpt::edge())
            .unwrap();

        Handler {
            poll,
            token,
            socket,
            recv_buffer: Vec::with_capacity(1024),
            send_buffer: Vec::with_capacity(1024),
            is_open: true,
        }
    }

    fn pull(&mut self) {
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

        self.poll.reregister(&self.socket, self.token, Ready::readable(), PollOpt::edge())
            .unwrap();
    }

    fn push(&mut self) {
        match self.socket.write_all(&self.send_buffer[..]) {
            Ok(_) => (),
            Err(_) => {
                self.is_open = false;
                return;
            }
        }
        self.send_buffer.clear();

        // Re-register for reading event == reuse existing connection
        self.poll.reregister(&self.socket, self.token, Ready::readable(), PollOpt::edge())
            .unwrap();
    }

    fn pick<T>(&mut self, f: fn(&Vec<u8>) -> (usize, Option<T>)) -> Option<T> {
        let (consumed, result_opt) = f(&self.recv_buffer);

        if consumed > 0 {
            let remaining = skip(consumed, self.recv_buffer.to_owned());
            self.recv_buffer = remaining;
        }

        result_opt
    }

    fn put<T>(&mut self, result: T, f: fn(T) -> Vec<u8>) {
        let mut bytes = f(result);
        self.send_buffer.append(&mut bytes);

        if !self.send_buffer.is_empty() {
            self.poll.reregister(&self.socket, self.token, Ready::writable(), PollOpt::edge() | PollOpt::oneshot())
                .unwrap();
        }
    }
}

fn main() {
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

    let mut events = Events::with_capacity(1024);
    loop {
        poll.poll(&mut events, None).unwrap();
        for event in &events {
            match event.token() {
                Token(0) => {
                    loop {
                        match listener.accept() {
                            Ok((socket, _)) => {
                                counter += 1;
                                let token = Token(counter);
                                handlers.insert(token, Handler::init(&poll, token, socket));
                            },
                            Err(ref e) if blocks(e) =>
                                break,
                            Err(_) => break
                        }
                    }
                },
                token if event.readiness().is_readable() => {
                    let handler = handlers.get_mut(&token).unwrap();
                    handler.pull();
                    let ready_opt = handler.pick(|bytes| {
                        let found = bytes
                            .windows(4)
                            .find(|window| is_double_crnl(*window))
                            .is_some();

                        if found {
                            (bytes.len(), Some(true))
                        } else {
                            (0, None)
                        }
                    });

                    if ready_opt.unwrap_or(false) {
                        handler.put(RESPONSE, |r: &str| r.as_bytes().to_owned().to_vec());
                    }

                    if !handler.is_open {
                        handlers.remove(&token);
                    }
                },
                token if event.readiness().is_writable() => {
                    let handler = handlers.get_mut(&token).unwrap();
                    handler.push();

                    if !handler.is_open {
                        handlers.remove(&token);
                    }
                },
                _ => unreachable!()
            }
        }
    }
}

