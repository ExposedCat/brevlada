use super::mail::Mail;
use crate::models::Account;
use anyhow::Result;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Connections(HashMap<String, (Mail, Instant)>);

impl Connections {
    pub fn execute<T>(
        &mut self,
        account: &Account,
        mut operation: impl FnMut(&mut Mail) -> Result<T>,
    ) -> Result<T> {
        self.0
            .retain(|_, (_, used)| used.elapsed() < Duration::from_secs(300));
        for attempt in 0..2 {
            if !self.0.contains_key(&account.path) {
                self.0.insert(
                    account.path.clone(),
                    (Mail::connect(account)?, Instant::now()),
                );
            }
            let (mail, used) = self.0.get_mut(&account.path).unwrap();
            *used = Instant::now();
            match operation(mail) {
                Ok(result) => return Ok(result),
                Err(error) => {
                    self.0.remove(&account.path);
                    if attempt == 1 {
                        return Err(error);
                    }
                }
            }
        }
        unreachable!()
    }
    pub fn execute_body<T>(
        &mut self,
        account: &Account,
        queue: &super::body_queue::BodyQueue,
        selection: u64,
        mut operation: impl FnMut(&mut Mail) -> Result<T>,
    ) -> Result<T> {
        loop {
            anyhow::ensure!(
                queue.wait_background(selection),
                "Message selection changed"
            );
            let epoch = queue.priority_epoch();
            let result =
                self.execute_body_priority(account, queue, selection, false, &mut operation);
            if result.is_err()
                && queue.current(selection)
                && (queue.priority_epoch() != epoch || !queue.current_work(selection, false))
            {
                continue;
            }
            return result;
        }
    }

    pub fn execute_body_priority<T>(
        &mut self,
        account: &Account,
        queue: &super::body_queue::BodyQueue,
        selection: u64,
        opened: bool,
        mut operation: impl FnMut(&mut Mail) -> Result<T>,
    ) -> Result<T> {
        for attempt in 0..2 {
            anyhow::ensure!(
                queue.current_work(selection, opened),
                "Message loading was superseded"
            );
            let mut active = None;
            if !self.0.contains_key(&account.path) {
                let mail = Mail::connect_with(account, |socket| {
                    active = queue.watch_work(selection, socket, opened);
                    anyhow::ensure!(active.is_some(), "Message selection changed");
                    Ok(())
                })?;
                self.0.insert(account.path.clone(), (mail, Instant::now()));
            }
            let (mail, used) = self.0.get_mut(&account.path).unwrap();
            if active.is_none() {
                active = queue.watch_work(selection, mail.cancellation_socket()?, opened);
            }
            anyhow::ensure!(active.is_some(), "Message selection changed");
            *used = Instant::now();
            let result = operation(mail);
            drop(active);
            match result {
                Ok(value) => return Ok(value),
                Err(error) => {
                    self.0.remove(&account.path);
                    if attempt == 1 || !queue.current_work(selection, opened) {
                        return Err(error);
                    }
                }
            }
        }
        unreachable!()
    }
}
