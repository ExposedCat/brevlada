use super::worker::BodyRequest;
use std::{
    collections::{HashMap, VecDeque},
    net::{Shutdown, TcpStream},
    sync::{Arc, Condvar, Mutex},
};

#[derive(Default)]
struct Pending {
    selection: u64,
    closed: bool,
    requests: VecDeque<BodyRequest>,
    sockets: HashMap<u64, TcpStream>,
    next_id: u64,
}

#[derive(Clone, Default)]
pub struct BodyQueue(Arc<(Mutex<Pending>, Condvar)>);

impl BodyQueue {
    pub fn select(&self, selection: u64) {
        let mut pending = self.0.0.lock().unwrap();
        pending.selection = selection;
        pending.requests.clear();
        for (_, socket) in pending.sockets.drain() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub fn current(&self, selection: u64) -> bool {
        let pending = self.0.0.lock().unwrap();
        !pending.closed && pending.selection == selection
    }

    pub fn push(&self, request: BodyRequest) {
        let mut pending = self.0.0.lock().unwrap();
        if !pending.closed && pending.selection == request.selection {
            if request.mark_read {
                pending.requests.push_front(request);
            } else {
                pending.requests.push_back(request);
            }
            self.0.1.notify_one();
        }
    }

    pub fn pop(&self) -> Option<BodyRequest> {
        let mut pending = self.0.0.lock().unwrap();
        loop {
            if pending.closed {
                return None;
            }
            if let Some(request) = pending.requests.pop_front() {
                return Some(request);
            }
            pending = self.0.1.wait(pending).unwrap();
        }
    }

    pub fn watch(&self, selection: u64, socket: TcpStream) -> Option<Active> {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed || pending.selection != selection {
            let _ = socket.shutdown(Shutdown::Both);
            return None;
        }
        pending.next_id += 1;
        let id = pending.next_id;
        pending.sockets.insert(id, socket);
        Some(Active {
            queue: self.clone(),
            id,
        })
    }

    pub fn close(&self) {
        let mut pending = self.0.0.lock().unwrap();
        pending.closed = true;
        for (_, socket) in pending.sockets.drain() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        self.0.1.notify_all();
    }
}

pub struct Active {
    queue: BodyQueue,
    id: u64,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.queue.0.0.lock().unwrap().sockets.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Account;
    use std::io::Read;

    fn request(selection: u64, uid: u32) -> BodyRequest {
        BodyRequest {
            account: Account {
                path: String::new(),
                email: String::new(),
                name: String::new(),
                host: String::new(),
                username: String::new(),
                port: 993,
                ssl: true,
                tls: false,
                oauth2: true,
            },
            folder: "INBOX".into(),
            uid,
            generation: 1,
            selection,
            mark_read: false,
        }
    }

    #[test]
    fn new_selection_discards_old_queue_and_rejects_late_requests() {
        let queue = BodyQueue::default();
        queue.select(1);
        queue.push(request(1, 10));
        queue.push(request(1, 11));
        queue.select(2);
        queue.push(request(1, 12));
        queue.push(request(2, 20));
        assert_eq!(queue.pop().unwrap().uid, 20);
        queue.close();
        assert!(queue.pop().is_none());
    }

    #[test]
    fn opens_messages_before_queued_previews() {
        let queue = BodyQueue::default();
        queue.select(1);
        queue.push(request(1, 10));
        queue.push(request(1, 11));
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 12)
        });
        assert_eq!(queue.pop().unwrap().uid, 12);
        assert_eq!(queue.pop().unwrap().uid, 10);
        assert_eq!(queue.pop().unwrap().uid, 11);
        queue.close();
    }

    #[test]
    fn switching_selection_interrupts_an_active_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let queue = BodyQueue::default();
        queue.select(1);
        let _active = queue.watch(1, socket).unwrap();
        queue.select(2);
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
}
