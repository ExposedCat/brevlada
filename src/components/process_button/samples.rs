use super::{
    Preview,
    queue::{Item, State},
};

pub(super) fn previews() -> [Preview; 2] {
    [
        Preview {
            name: "Syncing",
            percent: 64,
            current: "Personal account · Inbox",
            completed: "1,280 of 2,000 emails · 1 folder failed",
            items: vec![
                Item {
                    title: "Account discovery",
                    detail: "Personal and work accounts",
                    state: State::Done,
                },
                Item {
                    title: "Folders",
                    detail: "18 folders checked",
                    state: State::Done,
                },
                Item {
                    title: "Inbox · Personal account",
                    detail: "Downloading emails · 420 of 650",
                    state: State::Current,
                },
                Item {
                    title: "Archive · Personal account",
                    detail: "Connection timed out · 180 emails remaining",
                    state: State::Failed { retry_url: None },
                },
                Item {
                    title: "Inbox · Work account",
                    detail: "140 emails waiting",
                    state: State::Waiting,
                },
                Item {
                    title: "Sent · Work account",
                    detail: "100 emails waiting",
                    state: State::Waiting,
                },
                Item {
                    title: "Newsletters · Personal account",
                    detail: "70 emails waiting",
                    state: State::Waiting,
                },
            ],
        },
        Preview {
            name: "Unsubscribing",
            percent: 40,
            current: "Disney+ · disneyplus@mail.disneyplus.com",
            completed: "2 of 5 senders processed · 1 failed",
            items: vec![
                Item {
                    title: "Change.org",
                    detail: "Unsubscribed",
                    state: State::Done,
                },
                Item {
                    title: "Dygma",
                    detail: "Unsubscribe request failed",
                    state: State::Failed {
                        retry_url: Some("https://example.com/unsubscribe"),
                    },
                },
                Item {
                    title: "Disney+",
                    detail: "Sending unsubscribe request",
                    state: State::Current,
                },
                Item {
                    title: "StackShare Feed Digest",
                    detail: "feed@stackshare.io · waiting",
                    state: State::Waiting,
                },
                Item {
                    title: "GitKraken",
                    detail: "feedback@gitkraken.com · waiting",
                    state: State::Waiting,
                },
            ],
        },
    ]
}
