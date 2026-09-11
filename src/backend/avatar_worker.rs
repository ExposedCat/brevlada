use super::{
    avatar_queue::AvatarQueue,
    avatars::{self, now},
    storage::Storage,
    worker::Event,
};
use anyhow::Result;
use std::path::PathBuf;

pub fn start(path: PathBuf, queue: AvatarQueue, events: async_channel::Sender<Event>) {
    std::thread::spawn(move || {
        while let Some(email) = queue.pop() {
            let image = resolve(&path, &queue, &email).unwrap_or_default();
            if events.send_blocking(Event::Avatar(email, image)).is_err() {
                break;
            }
        }
    });
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
