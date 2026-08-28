//! Database unit of work. Domain settlement ports run inside the same transaction.

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub trait DomainSettlement {
    fn apply(&self, tx: &Transaction<'_>, failure: &crate::platform::failure::OperationalFailure) -> Result<()>;
}

pub struct UnitOfWork<'conn> {
    tx: Transaction<'conn>,
}

impl<'conn> UnitOfWork<'conn> {
    pub fn begin(conn: &'conn mut Connection) -> Result<Self> {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("begin immediate failure unit of work")?;
        Ok(Self { tx })
    }

    pub fn transaction(&self) -> &Transaction<'_> {
        &self.tx
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit().context("commit failure unit of work")
    }
}
