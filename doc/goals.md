Goals of web(socket) server design:

- abstract underlying network heavy lifting
- operate in clean domain-driven entities
- static and type-safe interface
- async and concurrent impl

--

Ideas:

actor model?

https://en.wikipedia.org/wiki/Actor_model
In response to a message it receives, an actor can: 
- make local decisions, 
- create more actors, 
- send more messages, and 
- determine how to respond to the next message received. 

Actors may modify their own private state, but can only affect each other indirectly through messaging.

```rust
// A = Address
// S = State
// M = Message
enum Action<A, S, M: Send> {
    // "make local decision[s]" - replace the state with the provided one
    Mutate { state: S },
    // "create [more] actor[s]" - create a new actor (initial state factory + receiver)
    Spawn { init: Fn() -> S, receive: Fn(S, M) -> Vec<Self> },
    // "send [more] message[s]" - send message to a target actor (source can be substituted)
    Send { target: A, source: A, message: M },
    // "determine how to respond to the next message received"
    Become { receive: Fn(S, M) -> Vec<Self> }
}

// Receive = given State and Message, decide what to do (list of actions an actor decides to make)

// The receive call can fail! Should it be represented in the type?

enum Event<A, S, S1, M: Send, M1: Send, E, E1> {
    Set { state: S },
    Run { init: Fn() -> S1, receive: Fn(S1, M1) -> Vec<Event<A, S1, _, M1, _, E1, _>> },
    Send { target: A, source: A, message: M },
    Recv { receive: Fn(S, M) -> Vec<Self> }
}

// Use https://crates.io/crates/bincode and deal with bytes? Easily scales up, but still no type safety.
// 
```

--

actor - as a model for async independent computational process?

types?: Input, State, Output

operations?: 
- receive next message from a mailbox
- send message to a target actor's mailbox
- create a new actor
- update internal state (~?= determine how to respond to the next message received)

```rust
enum Message {
    Ping(usize),
    Pong(usize)
}

struct Counter {
    value: usize
}

// Actor is an entity that can receive messages of type M
trait Actor<S, M: Send> {
    fn init(state: S) -> Self;
    fn receive(&mut self, message: M);
}

struct PingActor {
    counter: Counter    
}

impl Actor<Counter, Message> for PingActor {
    fn init(counter: Counter) -> Self {
        PingActor { counter }
    }

    fn receive(&mut self, message: Message) {
        self.counter.value += 1;
        
        let reply: Option<Message> = match message {
            Ping(n) => Some(Pong(n)),
            Pong(_) => None,
        };

        let sender = undefined!();
        reply.foreach(|r| sender.tell(r))        
    }
}

trait Address<M: Send> {
    fn send(message: M);
}

fn main() {
    let actor: Actor<Counter, Message> = ActorFactory::build(
        // how to create the initial state of an actor
        || Counter { value: 0 },
    );
}
```

if an actor has an address (String) - then it effectively looses any details about types, thus not type safe
if an actor has a typed interface (e.g. Sender<M>) - then it is unclear how to share it between actors