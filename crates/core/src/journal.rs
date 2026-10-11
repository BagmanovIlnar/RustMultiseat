//! Transaction journal used for start/stop rollback and crash recovery.

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::ids::{SeatId, SessionId};
use crate::seat::MultiseatConfig;

/// Opaque rollback handle returned by backends.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RollbackToken(pub String);

impl RollbackToken {
    /// Creates a new token from a string label.
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }
}

/// One applied step that may need compensation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalEntry {
    /// Host snapshot preserved before mutation.
    HostSnapshotSaved,
    /// Session created/connected for a seat.
    SessionCreated {
        /// Seat.
        seat_id: SeatId,
        /// Session.
        session_id: SessionId,
    },
    /// Session agent started.
    AgentStarted {
        /// Seat.
        seat_id: SeatId,
        /// Session.
        session_id: SessionId,
    },
    /// Display attached to a session.
    DisplayAttached {
        /// Seat.
        seat_id: SeatId,
        /// Backend rollback token.
        token: RollbackToken,
    },
    /// Input routes applied.
    InputRoutesApplied {
        /// Backend rollback token.
        token: RollbackToken,
    },
}

/// Durable operation journal for one start/stop transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OperationJournal {
    /// Schema version for on-disk format.
    pub schema_version: u32,
    /// Configuration being applied.
    pub config: MultiseatConfig,
    /// Applied steps in order.
    pub entries: Vec<JournalEntry>,
    /// Whether the start transaction completed successfully.
    pub completed: bool,
    /// Whether multiseat is considered active and needs clean stop on recover.
    pub active: bool,
}

/// Current on-disk journal schema.
pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

impl OperationJournal {
    /// Starts a journal for the given config.
    pub fn begin(config: MultiseatConfig) -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA_VERSION,
            config,
            entries: Vec::new(),
            completed: false,
            active: false,
        }
    }

    /// Records a successful step.
    pub fn push(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
    }

    /// Marks the journal as completed and active.
    pub fn mark_completed(&mut self) {
        self.completed = true;
        self.active = true;
    }

    /// Marks multiseat inactive after a clean stop.
    pub fn mark_inactive(&mut self) {
        self.active = false;
    }

    /// Returns entries in reverse order for rollback.
    pub fn rollback_order(&self) -> impl Iterator<Item = &JournalEntry> {
        self.entries.iter().rev()
    }

    /// True when crash recovery should run compensating actions.
    pub fn needs_recovery(&self) -> bool {
        self.active || !self.completed
    }
}

/// Persistence backend for [`OperationJournal`].
pub trait JournalStore {
    /// Loads a previously saved journal, if any.
    fn load(&self) -> CoreResult<Option<OperationJournal>>;

    /// Atomically replaces the durable journal.
    fn save(&self, journal: &OperationJournal) -> CoreResult<()>;

    /// Deletes the durable journal after a clean stop.
    fn clear(&self) -> CoreResult<()>;
}

/// No-op store used in unit tests.
#[derive(Debug, Default)]
pub struct MemoryJournalStore {
    /// In-memory contents.
    pub journal: std::cell::RefCell<Option<OperationJournal>>,
}

impl JournalStore for MemoryJournalStore {
    fn load(&self) -> CoreResult<Option<OperationJournal>> {
        Ok(self.journal.borrow().clone())
    }

    fn save(&self, journal: &OperationJournal) -> CoreResult<()> {
        *self.journal.borrow_mut() = Some(journal.clone());
        Ok(())
    }

    fn clear(&self) -> CoreResult<()> {
        *self.journal.borrow_mut() = None;
        Ok(())
    }
}

/// Helper that maps IO failures into [`CoreError::Journal`].
pub fn journal_io_err(err: impl std::fmt::Display) -> CoreError {
    CoreError::Journal(err.to_string())
}
