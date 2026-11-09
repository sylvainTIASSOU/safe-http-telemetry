//! Bounded asynchronous runtime primitives.
//!
//! This module contains no network client. Events must be policy-filtered and
//! redacted before they are pushed into the queue.

pub mod queue;
