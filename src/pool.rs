use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use std::thread::JoinHandle;
use std::sync::{Arc, Mutex};

type Runnable = Box<dyn FnOnce() + Send + 'static>;

enum Job {
    Task(Runnable),
    Stop,
}

pub struct ThreadPool {
    sender: Sender<Job>,
    workers: Vec<Worker>,
}

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        let (sender, receiver) = channel();
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            let worker = Worker::new(Arc::clone(&receiver));
            workers.push(worker);
        }

        ThreadPool {
            sender,
            workers,
        }
    }

    pub fn submit<F>(&mut self, f: F)
        where
            F: FnOnce() + Send + 'static
    {
        let job = Job::Task(Box::new(f));
        self.sender.send(job).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            self.sender.send(Job::Stop).unwrap();
        }

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

struct Worker {
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(receiver: Arc<Mutex<Receiver<Job>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let job = receiver.lock().unwrap().recv();
                match job {
                    Ok(Job::Task(f)) => f(),
                    Ok(Job::Stop) => break,
                    Err(_) => break
                }
            }
        });

        Worker {
            thread: Some(thread),
        }
    }
}