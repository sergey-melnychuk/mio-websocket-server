# mio-websocket-server
Async implementation of websocket server with mio and parser combinators

### Running locally

1. `RUST_LOG=debug cargo run`
1. Open `client.html` in browser
1. `[26/04/2020, 15:17:40] connected`
1. `[26/04/2020, 15:17:40] sent: hello!`
1. `[26/04/2020, 15:17:40] received 'ECHO: 'hello!'`

### Quick benchmarking

- n1-standard-8 (8 vCPUs, 30 GB memory)
- `wrk -d 30s -t 4 -c 128 http://127.0.0.1:9000/`

#### Single-thread (hard-coded response): [mio-tcp-server](https://github.com/sergey-melnychuk/mio-tcp-server)

```
Running 30s test @ http://127.0.0.1:8080/
  4 threads and 128 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency     1.16ms  139.76us   2.74ms   78.04%
    Req/Sec    27.49k     2.91k  119.25k    95.92%
  3284968 requests in 30.10s, 275.69MB read
Requests/sec: 109134.67
Transfer/sec:      9.16MB
```

#### Multi-threaded (hard-coded response)

```
Running 30s test @ http://127.0.0.1:9000/
  4 threads and 128 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   581.95us    1.15ms  28.56ms   97.82%
    Req/Sec    65.83k     5.14k   77.73k    79.15%
  7863843 requests in 30.04s, 659.96MB read
Requests/sec: 261810.66
Transfer/sec:     21.97MB
```

#### Multi-threader (request parsing + response serialization)

```
Running 30s test @ http://127.0.0.1:9000/
  4 threads and 128 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   806.85us  534.28us  16.80ms   98.72%
    Req/Sec    40.07k     3.81k   69.72k    53.12%
  4789399 requests in 30.10s, 424.78MB read
Requests/sec: 159116.81
Transfer/sec:     14.11MB
```
