//! Durable cursor recovery after connect, wake, resync, or periodic polling.
use std::collections::HashSet;
use std::future::Future;

use crate::{MqClient, SdkError};
use mq_core::{Message, ThreadId};

#[derive(Debug, PartialEq, Eq)]
pub enum CatchUpOutcome {
    /// The last read was empty; concurrent future publications are still possible.
    CaughtUp,
    /// More work may remain. Schedule another bounded pass.
    PageBudgetReached,
}

pub struct CatchUpSupervisor {
    thread_id: ThreadId,
    cursor: u64,
}

impl CatchUpSupervisor {
    /// Restore a cursor scoped to the current account and thread from durable storage.
    pub fn new(thread_id: ThreadId, durable_cursor: u64) -> Self {
        Self {
            thread_id,
            cursor: durable_cursor,
        }
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// Atomically persist each page and its cursor in `commit` before returning Ok.
    /// This is inbox acceptance, not execution of message instructions. A failed
    /// or cancelled commit may replay; persistence must deduplicate by message ID.
    /// Never advance a durable cursor from an SSE hint. Recreate this supervisor
    /// when account identity changes, and stop polling on authorization failure.
    pub async fn catch_up<F, Fut>(
        &mut self,
        client: &MqClient,
        max_pages: usize,
        mut commit: F,
    ) -> Result<CatchUpOutcome, SdkError>
    where
        F: FnMut(Vec<Message>, u64) -> Fut,
        Fut: Future<Output = Result<(), SdkError>>,
    {
        if !(1..=100).contains(&max_pages) {
            return Err(SdkError::Decode(
                "catch-up page budget must be 1..=100".into(),
            ));
        }
        for _ in 0..max_pages {
            let messages = client
                .read_messages(self.thread_id, self.cursor, 200)
                .await?;
            if messages.is_empty() {
                return Ok(CatchUpOutcome::CaughtUp);
            }
            if messages.len() > 200 {
                return Err(SdkError::Decode(
                    "catch-up page exceeds requested bound".into(),
                ));
            }
            let mut next = self.cursor;
            let mut identities = HashSet::with_capacity(messages.len());
            for message in &messages {
                // This endpoint returns complete thread history. A future
                // grant-filtered endpoint needs an explicit server cursor;
                // silently jumping gaps here would lose inbox messages.
                if message.thread_id != self.thread_id
                    || next.checked_add(1) != Some(message.seq)
                    || !identities.insert(message.message_id)
                {
                    return Err(SdkError::Decode(
                        "catch-up thread or sequence mismatch".into(),
                    ));
                }
                next = message.seq;
            }
            commit(messages, next).await?;
            self.cursor = next;
        }
        Ok(CatchUpOutcome::PageBudgetReached)
    }
}
