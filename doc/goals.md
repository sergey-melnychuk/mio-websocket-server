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

--

actor - as a model for async independent computational process?

types?: Input, State, Output

operations?: 
- receive next message from a mailbox
- send message to a target actor's mailbox
- create a new actor
- update internal state [~?= determine how to respond to the next message received]

```rust
enum Message {
    Ping(usize),
    Pong(usize)
}

struct State {
    n: usize
}

impl<M> State {
    fn process(self, m: M) -> Self {
        self
    }
}

fn main() {
    let actor: Actor<Message> = Actor::make(
        // how to create the state (out of thin air)
        || State(0),
        // how to update the state according to the message 
        |m: Message, s: State| s.process(m),
        || {}
    );
}

```