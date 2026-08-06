//! Mailbox — a Mailpit-style inbound email capture and viewer.
//!
//! This is a top-level feature that lives *outside* the cloud provider modules
//! (`aws`, `azure`, `gcp`). It runs a standalone SMTP server that accepts mail
//! from any SMTP client, parses it, and stores it for inspection via the
//! dashboard. AWS SES sends are also funneled into the same store so the inbox
//! shows both SMTP- and SES-originated mail.
//!
//! * [`smtp`]    — the tokio TCP listener + minimal SMTP state machine
//! * [`service`] — capture, parse (via `mail-parser`), and persist logic
//! * [`models`]  — the captured-message data shapes

pub mod models;
pub mod service;
pub mod smtp;
