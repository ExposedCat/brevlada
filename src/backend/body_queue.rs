use super::worker::BodyRequest;
use std::{
    collections::{HashMap, HashSet, VecDeque},
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
    priority_epoch: u64,
    active_opened: bool,
    // Keep priority until the UI has processed results and queued any next cards.
    awaiting: HashSet<(String, u32, bool)>,
    conversation_pending: usize,
    background: Option<super::sync_queue::SyncQueue>,
}

#[derive(Clone, Default)]
pub struct BodyQueue(Arc<(Mutex<Pending>, Condvar)>);

impl BodyQueue {
    fn foreground(pending: &Pending) -> bool {
        pending.active_opened || pending.conversation_pending > 0 || !pending.awaiting.is_empty()
    }
    pub fn with_background(background: super::sync_queue::SyncQueue) -> Self {
        let queue = Self::default();
        queue.0.0.lock().unwrap().background = Some(background);
        queue
    }

    fn update_priority(pending: &Pending) {
        if let Some(background) = &pending.background {
            background.pause_for_opened(Self::foreground(pending));
        }
    }

    pub fn select(&self, selection: u64) {
        let mut pending = self.0.0.lock().unwrap();
        pending.selection = selection;
        pending.requests.clear();
        pending.active_opened = false;
        pending.awaiting.clear();
        pending.conversation_pending = 0;
        for (_, socket) in pending.sockets.drain() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        Self::update_priority(&pending);
        self.0.1.notify_all();
    }

    pub fn current(&self, selection: u64) -> bool {
        let pending = self.0.0.lock().unwrap();
        !pending.closed && pending.selection == selection
    }

    pub fn current_work(&self, selection: u64, opened: bool) -> bool {
        let pending = self.0.0.lock().unwrap();
        !pending.closed && pending.selection == selection && (opened || !Self::foreground(&pending))
    }

    pub fn wait_background(&self, selection: u64) -> bool {
        let mut pending = self.0.0.lock().unwrap();
        loop {
            if pending.closed || pending.selection != selection {
                return false;
            }
            if !Self::foreground(&pending) {
                return true;
            }
            pending = self.0.1.wait(pending).unwrap();
        }
    }

    pub fn priority_epoch(&self) -> u64 {
        self.0.0.lock().unwrap().priority_epoch
    }

    pub fn conversation(&self, selection: u64, pending_lookup: bool) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed || pending.selection != selection {
            return;
        }
        if pending_lookup && !Self::foreground(&pending) {
            Self::interrupt_background(&mut pending);
        }
        if pending_lookup {
            pending.conversation_pending += 1;
        } else {
            pending.conversation_pending = pending.conversation_pending.saturating_sub(1);
        }
        Self::update_priority(&pending);
        self.0.1.notify_all();
    }

    fn interrupt_background(pending: &mut Pending) {
        pending.priority_epoch += 1;
        for (_, socket) in pending.sockets.drain() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub fn handled(&self, selection: u64, folder: &str, uid: u32, related: bool) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.selection != selection {
            return;
        }
        pending.awaiting.remove(&(folder.to_owned(), uid, related));
        Self::update_priority(&pending);
        self.0.1.notify_all();
    }

    pub fn push(&self, request: BodyRequest) {
        let mut pending = self.0.0.lock().unwrap();
        if !pending.closed && pending.selection == request.selection {
            if request.opened() {
                // Interrupt previews or history lookups already using this queue.
                if !Self::foreground(&pending) {
                    Self::interrupt_background(&mut pending);
                }
                if !pending
                    .awaiting
                    .insert((request.folder.clone(), request.uid, request.related))
                {
                    return;
                }
                let position = pending
                    .requests
                    .iter()
                    .position(|queued| !queued.opened())
                    .unwrap_or(pending.requests.len());
                pending.requests.insert(position, request);
            } else {
                pending.requests.push_back(request);
            }
            Self::update_priority(&pending);
            self.0.1.notify_all();
        }
    }

    pub fn pop(&self) -> Option<BodyRequest> {
        let mut pending = self.0.0.lock().unwrap();
        loop {
            if pending.closed {
                return None;
            }
            if pending
                .requests
                .front()
                .is_some_and(|request| request.opened() || !Self::foreground(&pending))
                && let Some(request) = pending.requests.pop_front()
            {
                pending.active_opened = request.opened();
                return Some(request);
            }
            pending = self.0.1.wait(pending).unwrap();
        }
    }

    #[cfg(test)]
    pub fn watch(&self, selection: u64, socket: TcpStream) -> Option<Active> {
        self.watch_work(selection, socket, false)
    }

    pub fn watch_work(&self, selection: u64, socket: TcpStream, opened: bool) -> Option<Active> {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed
            || pending.selection != selection
            || (!opened && Self::foreground(&pending))
        {
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

    pub fn complete(&self, selection: u64) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.selection == selection {
            pending.active_opened = false;
            Self::update_priority(&pending);
            self.0.1.notify_all();
        }
    }

    pub fn order(&self, order: &[(String, u32)]) {
        let mut pending = self.0.0.lock().unwrap();
        pending.requests.make_contiguous().sort_by_key(|request| {
            (
                !request.opened(),
                order
                    .iter()
                    .position(|(folder, uid)| folder == &request.folder && *uid == request.uid)
                    .unwrap_or(usize::MAX),
            )
        });
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
                smtp: None,
            },
            folder: "INBOX".into(),
            uid,
            generation: 1,
            selection,
            mark_read: false,
            load_inline: false,
            related: false,
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
            related: false,
            ..request(1, 12)
        });
        assert_eq!(queue.pop().unwrap().uid, 12);
        queue.complete(1);
        queue.handled(1, "INBOX", 12, false);
        assert_eq!(queue.pop().unwrap().uid, 10);
        assert_eq!(queue.pop().unwrap().uid, 11);
        queue.close();
    }

    #[test]
    fn opened_messages_load_fifo_pause_background_and_preempt_previews() {
        let background = super::super::sync_queue::SyncQueue::default();
        let queue = BodyQueue::with_background(background.clone());
        queue.select(1);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let _preview = queue.watch(1, socket).unwrap();
        queue.push(request(1, 99));
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 10)
        });
        queue.push(BodyRequest {
            related: true,
            ..request(1, 11)
        });
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        assert!(!background.cancellation.current(0));
        assert!(!queue.current_work(1, false));
        assert_eq!(queue.pop().unwrap().uid, 10);
        queue.complete(1);
        assert!(!background.cancellation.current(0));
        queue.handled(1, "INBOX", 10, false);
        assert_eq!(queue.pop().unwrap().uid, 11);
        queue.complete(1);
        assert!(!background.cancellation.current(0));
        queue.handled(1, "INBOX", 11, true);
        assert!(background.cancellation.current(0));
        assert_eq!(queue.pop().unwrap().uid, 99);
        queue.close();
    }

    #[test]
    fn conversation_interrupts_running_sync_and_holds_priority_until_ui_finishes() {
        use super::super::sync_queue::{Job, SyncQueue, Task};
        use std::{sync::mpsc, time::Duration};

        let background = SyncQueue::default();
        background.push(Job {
            account: request(1, 0).account,
            folder: "INBOX".into(),
            task: Task::Scan,
        });
        let running = background.pop().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let _active_sync = background.cancellation.watch(0, socket).unwrap();
        let queue = BodyQueue::with_background(background.clone());
        queue.select(1);

        // Pause as soon as the conversation is opened, before cache lookup returns.
        queue.conversation(1, true);
        queue.conversation(1, true);
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        background.finish(&running, false);
        assert_eq!(background.status().unwrap().failed, 0);
        let (sent, received) = mpsc::channel();
        let worker_queue = background.clone();
        let worker = std::thread::spawn(move || sent.send(worker_queue.pop()).unwrap());
        let still_paused = || {
            assert!(matches!(
                received.recv_timeout(Duration::from_millis(30)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
        };
        still_paused();

        // The cache result queues the visible cards before releasing its hold.
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 10)
        });
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 11)
        });
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 10)
        });
        assert_eq!(queue.0.0.lock().unwrap().requests.len(), 2);
        queue.conversation(1, false);
        queue.conversation(1, false);
        assert_eq!(queue.pop().unwrap().uid, 10);
        queue.complete(1);
        still_paused();
        queue.handled(1, "INBOX", 10, false);
        still_paused();
        assert_eq!(queue.pop().unwrap().uid, 11);
        queue.complete(1);
        // Worker completion must not resume sync ahead of the UI result handler.
        still_paused();
        queue.handled(1, "INBOX", 11, false);

        let resumed = received
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(resumed.folder, running.folder);
        background.finish(&resumed, true);
        worker.join().unwrap();
        queue.close();
        background.close();
    }

    #[test]
    fn switching_conversations_clears_holds_and_ignores_stale_results() {
        let background = super::super::sync_queue::SyncQueue::default();
        let queue = BodyQueue::with_background(background.clone());
        queue.select(1);
        queue.conversation(1, true);
        queue.push(BodyRequest {
            mark_read: true,
            ..request(1, 10)
        });
        queue.select(2);
        assert!(background.cancellation.current(0));
        queue.conversation(2, true);
        queue.conversation(1, false);
        queue.handled(1, "INBOX", 10, false);
        queue.complete(1);
        assert!(!background.cancellation.current(0));
        // Also covers a failed lookup, which releases without enqueuing bodies.
        queue.conversation(2, false);
        assert!(background.cancellation.current(0));
        queue.close();
        background.close();
    }

    #[test]
    fn reordered_opened_messages_follow_the_visible_card_order() {
        let queue = BodyQueue::default();
        queue.select(1);
        for uid in [3, 1, 2] {
            queue.push(BodyRequest {
                mark_read: true,
                ..request(1, uid)
            });
        }
        queue.order(&[
            ("INBOX".into(), 1),
            ("INBOX".into(), 2),
            ("INBOX".into(), 3),
        ]);
        assert_eq!(queue.pop().unwrap().uid, 1);
        assert_eq!(queue.pop().unwrap().uid, 2);
        assert_eq!(queue.pop().unwrap().uid, 3);
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
