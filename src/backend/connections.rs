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
        for attempt in 0..2 {
            anyhow::ensure!(queue.current(selection), "Message selection changed");
            let mut active = None;
            if !self.0.contains_key(&account.path) {
                let mail = Mail::connect_with(account, |socket| {
                    active = queue.watch(selection, socket);
                    anyhow::ensure!(active.is_some(), "Message selection changed");
                    Ok(())
                })?;
                self.0.insert(account.path.clone(), (mail, Instant::now()));
            }
            let (mail, used) = self.0.get_mut(&account.path).unwrap();
            if active.is_none() {
                active = queue.watch(selection, mail.cancellation_socket()?);
            }
            anyhow::ensure!(active.is_some(), "Message selection changed");
            *used = Instant::now();
            let result = operation(mail);
            drop(active);
            match result {
                Ok(value) => return Ok(value),
                Err(error) => {
                    self.0.remove(&account.path);
                    if attempt == 1 || !queue.current(selection) {
                        return Err(error);
                    }
                }
            }
        }
        unreachable!()
    }
}
