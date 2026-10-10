use crate::{
    backend::{
        parser,
        search::{self, Request, Scope},
        storage::Storage,
    },
    components,
    models::{self, Message},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

const ACCOUNT: &str = "bench@brevlada.test";
const FOLDER: &str = "INBOX";
const QUERIES: [&str; 5] = ["invoice", "meeting", "zz-no-match", "alice", "the"];

fn report(name: &str, started: Instant, count: usize) {
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    println!("{{\"name\":\"{name}\",\"ms\":{ms:.3},\"count\":{count}}}");
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn repeat(name: &str, times: usize, mut work: impl FnMut() -> usize) {
    let mut count = 0;
    let samples = (0..times)
        .map(|_| {
            let started = Instant::now();
            count = work();
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    println!(
        "{{\"name\":\"{name}\",\"ms\":{:.3},\"count\":{count}}}",
        median(samples)
    );
}

#[test]
#[ignore = "Benchmark: set BREVLADA_BENCH_CORPUS to a directory of .eml files and run with --release --nocapture"]
fn storage_search_and_rendering() -> anyhow::Result<()> {
    let corpus = PathBuf::from(std::env::var("BREVLADA_BENCH_CORPUS")?);
    let database = std::env::temp_dir().join("brevlada-bench.db");
    let database = database.as_path();
    let mut paths: Vec<_> = std::fs::read_dir(&corpus)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    let files: Vec<Vec<u8>> = paths.iter().map(std::fs::read).collect::<Result<_, _>>()?;

    let started = Instant::now();
    let full: Vec<Message> = files
        .iter()
        .enumerate()
        .map(|(index, bytes)| parser::parse(index as u32 + 1, bytes, index % 3 == 0, true))
        .collect::<anyhow::Result<_>>()?;
    report("parse", started, full.len());

    let headers: Vec<Message> = files
        .iter()
        .enumerate()
        .map(|(index, bytes)| parser::parse(index as u32 + 1, bytes, index % 3 == 0, false))
        .collect::<anyhow::Result<_>>()?;
    let uids: Vec<u32> = headers.iter().map(|message| message.uid).collect();

    let _ = std::fs::remove_file(database);
    let mut storage = Storage::open(database)?;
    let started = Instant::now();
    storage.store_folders(ACCOUNT, &[FOLDER.to_owned()])?;
    let revision = storage.read_revision()?;
    storage.reconcile_since(ACCOUNT, FOLDER, 1, headers.clone(), &uids, revision)?;
    report("index_headers", started, headers.len());
    let started = Instant::now();
    for message in &full {
        storage.store_body(ACCOUNT, FOLDER, Some(1), message)?;
    }
    report("index_bodies", started, full.len());

    let mut list = Vec::new();
    repeat("folder_list", 5, || {
        list = storage.sender_headers(ACCOUNT, FOLDER).unwrap();
        list.len()
    });

    let mut senders: Vec<(String, usize)> = models::senders::groups(&list, "")
        .into_iter()
        .map(|group| (models::senders::key(&group[0]), group.len()))
        .collect();
    senders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    senders.truncate(100);
    repeat("sender_pages", 3, || {
        senders
            .iter()
            .map(|(sender, _)| {
                storage
                    .sender_page(ACCOUNT, FOLDER, sender, None, 51)
                    .unwrap()
                    .len()
            })
            .sum()
    });

    repeat("message_lookup", 3, || {
        (1..=2000u32)
            .map(|step| step * 7919 % uids.len() as u32 + 1)
            .filter(|uid| storage.message(ACCOUNT, FOLDER, *uid).unwrap().is_some())
            .count()
    });

    repeat("threads", 5, || models::threads(&list, "").len());
    repeat("sender_groups", 5, || {
        models::senders::groups(&list, "").len()
    });

    let html: Vec<&Message> = full
        .iter()
        .filter(|message| !message.body_html.is_empty())
        .take(1000)
        .collect();
    repeat("render_letter", 3, || {
        html.iter()
            .map(|message| {
                let mut message = (*message).clone();
                message.prepare_display();
                components::html::document(&message, true, false).len()
            })
            .sum()
    });

    let (search, results) = search::Search::start();
    let snapshot = Arc::new(list.clone());
    let mut wait = |request: Request| {
        search.submit(request);
        loop {
            let response = results.recv_blocking().unwrap();
            if response.complete && search.accepts(&response) {
                return response.groups.len();
            }
        }
    };
    repeat("search_memory", 3, || {
        QUERIES
            .iter()
            .map(|query| {
                wait(Request::new(
                    false,
                    query.to_string(),
                    None,
                    None,
                    snapshot.clone(),
                ))
            })
            .sum()
    });
    let cache_path = database.to_path_buf();
    repeat("search_cache", 3, || {
        QUERIES
            .iter()
            .map(|query| {
                let mut request =
                    Request::new(false, query.to_string(), None, None, Arc::new(Vec::new()));
                request.scope = Some(Scope {
                    current: None,
                    folder: None,
                    accounts: vec![ACCOUNT.to_owned()],
                    cache_path: cache_path.clone(),
                    revision: 1,
                });
                wait(request)
            })
            .sum()
    });
    Ok(())
}
