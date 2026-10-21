use super::{
    avatar_queue::AvatarQueue,
    avatars::{self, now},
    storage::Storage,
    worker::Event,
};
use anyhow::Result;
use std::path::PathBuf;

pub fn start(path: PathBuf, queue: AvatarQueue, events: async_channel::Sender<Event>) {
    for priority_only in [true, false, false] {
        let path = path.clone();
        let pending = queue.clone();
        let events = events.clone();
        std::thread::spawn(move || {
            while let Some(email) = pending.pop_lookup(priority_only) {
                let event = match resolve(&path, &pending, &email) {
                    Ok(image) => Event::Avatar(email.clone(), image),
                    Err(_) => Event::AvatarError(email.clone()),
                };
                pending.finish_lookup(&email);
                if events.send_blocking(event).is_err() {
                    break;
                }
            }
        });
    }
    std::thread::spawn(move || {
        while let Some(email) = queue.pop() {
            let cached = Storage::open(&path)
                .and_then(|storage| storage.avatar(&email))
                .ok()
                .flatten();
            if deliver_cached(&email, cached, &queue, &events).is_err() {
                break;
            }
        }
        queue.close();
    });
}

fn deliver_cached(
    email: &str,
    cached: Option<super::storage::avatar::Cached>,
    lookups: &AvatarQueue,
    events: &async_channel::Sender<Event>,
) -> Result<()> {
    if let Some(cached) = cached {
        let fresh = avatars::is_fresh(cached.image.is_some(), cached.fetched_at, now());
        if cached.image.is_some() || fresh {
            events.send_blocking(Event::Avatar(email.to_owned(), cached.image))?;
        }
        if fresh {
            lookups.finish_cached(email);
            return Ok(());
        }
    }
    lookups.lookup(email.to_owned());
    Ok(())
}

/// Answers from the cache when possible, otherwise looks the sender up and
/// records the result — including "this sender has no avatar" — so that neither
/// Google nor Gravatar is asked again for a while.
fn resolve(path: &std::path::Path, queue: &AvatarQueue, email: &str) -> Result<Option<Vec<u8>>> {
    let storage = Storage::open(path)?;
    if let Some(cached) = storage.avatar(email)?
        && avatars::is_fresh(cached.image.is_some(), cached.fetched_at, now())
    {
        return Ok(cached.image);
    }
    // A failure here means the services were unreachable rather than that the
    // sender has no avatar, so the cache is left untouched and the lookup is
    // retried the next time the sender is shown.
    let fetched = avatars::fetch(&queue.credentials(), email)?;
    if !fetched.unauthorized.is_empty() {
        queue.unauthorized(&fetched.unauthorized);
    }
    storage.store_avatar(email, fetched.source, fetched.image.as_deref())?;
    Ok(fetched.image)
}

#[cfg(test)]
mod tests {
    use super::{super::avatars::Source, *};

    #[test]
    fn cached_icons_do_not_wait_for_pending_network_lookups() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage
            .store_avatar("cached@example.com", Source::Favicon, Some(b"cached icon"))
            .unwrap();
        storage
            .store_avatar("missing@example.com", Source::None, None)
            .unwrap();
        let lookups = AvatarQueue::default();
        let (events, received) = async_channel::unbounded();
        for email in [
            "network@example.com",
            "cached@example.com",
            "missing@example.com",
        ] {
            deliver_cached(email, storage.avatar(email).unwrap(), &lookups, &events).unwrap();
        }
        let Event::Avatar(email, image) = received.try_recv().unwrap() else {
            panic!("Expected cached icon");
        };
        assert_eq!(email, "cached@example.com");
        assert_eq!(image.as_deref(), Some(b"cached icon".as_slice()));
        let Event::Avatar(email, image) = received.try_recv().unwrap() else {
            panic!("Expected cached miss");
        };
        assert_eq!(email, "missing@example.com");
        assert!(image.is_none());
        assert_eq!(lookups.pop_lookup(false).unwrap(), "network@example.com");
        assert!(received.try_recv().is_err());
        lookups.close();
        assert!(lookups.pop_lookup(false).is_none());
    }

    #[test]
    fn expired_icons_are_shown_before_their_network_refresh() {
        let lookups = AvatarQueue::default();
        let (events, received) = async_channel::unbounded();
        deliver_cached(
            "cached@example.com",
            Some(super::super::storage::avatar::Cached {
                image: Some(b"saved logo".to_vec()),
                fetched_at: now() - avatars::TTL.as_secs() as i64 - 1,
            }),
            &lookups,
            &events,
        )
        .unwrap();
        let Event::Avatar(email, image) = received.try_recv().unwrap() else {
            panic!("Expected saved icon before refresh");
        };
        assert_eq!(email, "cached@example.com");
        assert_eq!(image.as_deref(), Some(b"saved logo".as_slice()));
        assert_eq!(lookups.pop_lookup(false).unwrap(), "cached@example.com");
        lookups.close();
    }

    #[test]
    fn serves_cached_images_and_cached_misses_without_the_network() {
        let path = std::env::temp_dir().join(format!(
            "brevlada-avatar-{}-{}.db",
            std::process::id(),
            now()
        ));
        let storage = Storage::open(&path).unwrap();
        let queue = AvatarQueue::default();
        storage
            .store_avatar(
                "ada@example.com",
                Source::Google,
                Some(b"\x89PNG\r\n\x1a\n"),
            )
            .unwrap();
        assert_eq!(
            resolve(&path, &queue, "ada@example.com")
                .unwrap()
                .as_deref(),
            Some(b"\x89PNG\r\n\x1a\n".as_slice())
        );
        // A remembered miss resolves to initials without contacting anyone.
        storage
            .store_avatar("grace@example.com", Source::None, None)
            .unwrap();
        assert!(
            resolve(&path, &queue, "grace@example.com")
                .unwrap()
                .is_none()
        );
        drop(storage);
        std::fs::remove_file(path).unwrap();
    }
}
