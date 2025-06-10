use crate::queue::Queue;

pub struct Uring<Sqe, Rqe> {
    send_queue: Queue<Sqe>,
    recv_queue: Queue<Rqe>,
}

impl<Sqe, Rqe> Uring<Sqe, Rqe> {
    pub fn send(&mut self, sqe: Sqe) {
        self.send_queue.enqueue(sqe);
    }

    pub fn recv(&mut self) -> Option<Rqe> {
        self.recv_queue.dequeue()
    }
}
