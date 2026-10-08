use super::Message;
use std::collections::HashMap;

pub fn merge<'a>(messages: &mut [Message], previous: impl Iterator<Item = &'a Message>) {
    merge_into(messages.iter_mut(), previous);
}

pub fn merge_into<'a, 'b>(
    messages: impl Iterator<Item = &'b mut Message>,
    previous: impl Iterator<Item = &'a Message>,
) {
    let mut newest = HashMap::new();
    for message in previous {
        let key = (
            message.uid,
            message.message_id.as_str(),
            message.uid_validity,
        );
        let entry = newest.entry(key).or_insert((message, message));
        if message.read_revision > entry.0.read_revision {
            entry.0 = message;
        }
        if message.flag_revision > entry.1.flag_revision {
            entry.1 = message;
        }
    }
    for message in messages {
        let key = (
            message.uid,
            message.message_id.as_str(),
            message.uid_validity,
        );
        let old = newest.get(&key).or_else(|| {
            message
                .uid_validity
                .and_then(|_| newest.get(&(message.uid, message.message_id.as_str(), None)))
        });
        if let Some((read, flag)) = old {
            if read.read_revision > message.read_revision {
                message.is_read = read.is_read;
                message.read_revision = read.read_revision;
            }
            if flag.flag_revision > message.flag_revision {
                message.is_flagged = flag.is_flagged;
                message.flag_revision = flag.flag_revision;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(read: bool, revision: i64) -> Message {
        Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "id".into(),
            is_read: read,
            read_revision: revision,
            ..Default::default()
        }
    }

    #[test]
    fn preserves_newest_read_state_from_all_snapshots() {
        let snapshots = [message(false, 1), message(true, 3), message(false, 2)];
        let mut incoming = [message(false, 2)];
        merge(&mut incoming, snapshots.iter());
        assert!(incoming[0].is_read);
        assert_eq!(incoming[0].read_revision, 3);
        incoming[0] = message(false, 4);
        merge(&mut incoming, snapshots.iter());
        assert!(!incoming[0].is_read);
        assert_eq!(incoming[0].read_revision, 4);
    }

    #[test]
    fn does_not_merge_reused_uids_or_changed_mailboxes() {
        let previous = message(true, 3);
        let mut incoming = [message(false, 1), message(false, 1), message(false, 1)];
        incoming[0].uid = 2;
        incoming[1].message_id = "replacement".into();
        incoming[2].uid_validity = Some(8);
        merge(&mut incoming, std::iter::once(&previous));
        assert!(incoming.iter().all(|message| !message.is_read));
    }

    #[test]
    fn merges_read_and_star_revisions_independently() {
        let read = message(true, 4);
        let star = Message {
            is_flagged: true,
            flag_revision: 5,
            ..message(false, 2)
        };
        let mut incoming = [message(false, 1)];
        merge(&mut incoming, [read, star].iter());
        assert!(incoming[0].is_read && incoming[0].is_flagged);
        assert_eq!(incoming[0].read_revision, 4);
        assert_eq!(incoming[0].flag_revision, 5);
        let unstar = Message {
            flag_revision: 6,
            ..message(false, 1)
        };
        merge(&mut incoming, std::iter::once(&unstar));
        assert!(incoming[0].is_read && !incoming[0].is_flagged);
    }

    #[test]
    fn old_cache_defaults_to_revision_zero() {
        let mut data = serde_json::to_value(message(false, 0)).unwrap();
        data.as_object_mut().unwrap().remove("read_revision");
        let message: Message = serde_json::from_value(data).unwrap();
        assert_eq!(message.read_revision, 0);
    }
}
