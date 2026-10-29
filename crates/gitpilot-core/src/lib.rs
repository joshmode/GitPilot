//! GitPilot's domain, persistence and application service layer.
//!
//! The desktop crate deliberately depends on this crate rather than issuing
//! network or database calls from widgets. All external mutations pass through
//! permission checks in [`permissions`] and are audited by [`store`].

pub mod ai;
pub mod commands;
pub mod credentials;
pub mod demo;
pub mod diff;
pub mod domain;
pub mod error;
pub mod github;
pub mod intelligence;
pub mod jobs;
pub mod knowledge;
pub mod local_git;
pub mod logging;
pub mod permissions;
pub mod review;
pub mod service;
pub mod store;
pub mod workflows;

pub use ai::ProviderKind;
pub use domain::*;
pub use error::{GitPilotError, Result};
pub use jobs::{JobHandler, JobRunner};
pub use review::{PublishMode, ReviewDepth, ReviewEngine, ReviewPolicy};
pub use service::{AppService, Bootstrap, PrQuery, UsageSummary};
pub use workflows::{Action, Event, Trigger, WorkflowEngine, WorkflowOutcome};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
