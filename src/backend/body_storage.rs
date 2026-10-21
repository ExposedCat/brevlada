use super::{Storage, messages};
use crate::{backend::parser::normalize_message_id, models::Message};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

impl Storage {
    /// Finds the same immutable message in another mailbox or the local sent cache.
    /// `store_body` merges it with the destination's current flags and mailbox identity.
    pub fn cached_body_copy(
        &self,
        account: &str,
        message: &Message,
        inline: bool,
    ) -> Result<Option<Message>> {
        let key = normalize_message_id(&message.message_id);
        if key.is_empty() || message.is_draft {
            return Ok(None);
        }
        let mut copy = self
            .0
            .prepare_cached(concat!(
                messages::select!(),
                " WHERE h.account_id=?1
                  AND trim(h.message_id, '<> ' || char(9) || char(10) || char(13))=?2
                  AND h.body_loaded AND NOT h.is_draft
                  AND (NOT ?3 OR h.inline_media_loaded OR instr(coalesce(b.body_html, ''), 'cid:')=0)
                  ORDER BY h.inline_media_loaded DESC, h.id LIMIT 1"
            ))?
            .query_row(params![account, key, inline], messages::decode)
            .optional()?;
        if let Some(copy) = &mut copy {
            copy.uid = message.uid;
            copy.uid_validity = message.uid_validity;
            copy.message_id = message.message_id.clone();
        }
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{InlineMedia, attachment};

    fn complete() -> Message {
        Message {
            uid: 7116,
            message_id: " \t<same@example.com>\r\n".into(),
            subject: "Partnership".into(),
            sender: "Me <me@example.com>".into(),
            recipients: "other@example.com".into(),
            reply_to: "replies@example.com".into(),
            cc: "copy@example.com".into(),
            body_text: "Already downloaded".into(),
            body_html: "<p>Already downloaded</p><img src=\"cid:logo\">".into(),
            attachments: vec!["proposal.pdf".into()],
            attachment_details: vec![attachment::Details {
                mime: "application/pdf".into(),
                size: 22_000_000,
                calendar_rendered: None,
            }],
            inline_media: vec![InlineMedia {
                content_id: "logo".into(),
                mime: "image/png".into(),
                data: "cached-image".into(),
            }],
            body_loaded: true,
            inline_media_loaded: true,
            ..Default::default()
        }
    }

    #[test]
    fn reuses_other_folders_body_without_copying_their_uid_validity_or_flags() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let source = complete();
        let target = Message {
            uid: 331,
            message_id: "same@example.com".into(),
            sender: "me@example.com".into(),
            recipients: source.recipients.clone(),
            is_read: true,
            is_flagged: true,
            is_spam: true,
            body_text: "Incomplete legacy cache".into(),
            ..Default::default()
        };
        storage
            .reconcile("me", "All Mail", 17, vec![source.clone()], &[7116])
            .unwrap();
        storage
            .reconcile("me", "Sent", 23, vec![target.clone()], &[331])
            .unwrap();
        let fetched = storage
            .cached_body_copy("me", &target, true)
            .unwrap()
            .unwrap();
        let stored = storage
            .store_body("me", "Sent", Some(23), &fetched)
            .unwrap()
            .unwrap();
        assert_eq!(stored.uid, 331);
        assert_eq!(stored.uid_validity, Some(23));
        assert_eq!(stored.message_id, target.message_id);
        assert_eq!(stored.sender, target.sender);
        assert_eq!(stored.recipients, target.recipients);
        assert!(stored.is_read && stored.is_flagged && stored.is_spam && !stored.is_draft);
        assert!(stored.body_loaded && stored.inline_media_loaded);
        assert_eq!(stored.body_text, source.body_text);
        assert_eq!(stored.body_html, source.body_html);
        assert_eq!(stored.reply_to, source.reply_to);
        assert_eq!(stored.cc, source.cc);
        assert_eq!(stored.attachments, source.attachments);
        assert_eq!(stored.attachment_details, source.attachment_details);
        assert_eq!(stored.inline_media, source.inline_media);

        // The destination keeps its own complete copy if the source goes away.
        storage.inventory("me", "All Mail", 17, &[]).unwrap();
        assert_eq!(storage.message("me", "Sent", 331).unwrap().unwrap(), stored);
        // A result from before a UIDVALIDITY reset must not replace a new message.
        storage
            .reconcile(
                "me",
                "Sent",
                24,
                vec![Message {
                    message_id: "replacement@example.com".into(),
                    ..target
                }],
                &[331],
            )
            .unwrap();
        assert!(
            storage
                .store_body("me", "Sent", Some(23), &fetched)
                .unwrap()
                .is_none()
        );
        assert!(
            !storage
                .message("me", "Sent", 331)
                .unwrap()
                .unwrap()
                .body_loaded
        );
    }

    #[test]
    fn ignores_incomplete_bodies_other_accounts_drafts_and_missing_or_different_ids() {
        let source = complete();
        let target = Message {
            message_id: "same@example.com".into(),
            ..Default::default()
        };
        for (account, source, target) in [
            (
                "me",
                Message {
                    body_loaded: false,
                    ..source.clone()
                },
                target.clone(),
            ),
            ("other", source.clone(), target.clone()),
            (
                "me",
                Message {
                    is_draft: true,
                    ..source.clone()
                },
                target.clone(),
            ),
            (
                "me",
                source.clone(),
                Message {
                    is_draft: true,
                    ..target.clone()
                },
            ),
            (
                "me",
                source.clone(),
                Message {
                    message_id: String::new(),
                    ..target.clone()
                },
            ),
            (
                "me",
                source.clone(),
                Message {
                    message_id: "different@example.com".into(),
                    ..target
                },
            ),
        ] {
            let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
            storage.store(account, "All Mail", &source).unwrap();
            assert!(
                storage
                    .cached_body_copy("me", &target, true)
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn requires_cached_inline_images_for_opened_html_and_prefers_the_complete_copy() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let full = complete();
        let partial = Message {
            inline_media_loaded: false,
            inline_media: vec![],
            ..full.clone()
        };
        let target = Message {
            uid: 331,
            message_id: "same@example.com".into(),
            ..Default::default()
        };
        storage.store("me", "All Mail", &partial).unwrap();
        assert!(
            storage
                .cached_body_copy("me", &target, true)
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .cached_body_copy("me", &target, false)
                .unwrap()
                .is_some()
        );
        storage.store("me", "Archive", &full).unwrap();
        for inline in [true, false] {
            let copy = storage
                .cached_body_copy("me", &target, inline)
                .unwrap()
                .unwrap();
            assert_eq!(copy.inline_media, full.inline_media);
            assert!(copy.inline_media_loaded);
        }
        storage
            .store(
                "me",
                "Plain",
                &Message {
                    message_id: "plain@example.com".into(),
                    body_html: String::new(),
                    ..partial
                },
            )
            .unwrap();
        assert!(
            storage
                .cached_body_copy(
                    "me",
                    &Message {
                        message_id: "plain@example.com".into(),
                        ..target
                    },
                    true
                )
                .unwrap()
                .is_some()
        );
    }
}
