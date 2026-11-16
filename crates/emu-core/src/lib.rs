//! Core library for the AWS Emulator TUI.
//!
//! `ApiError` deliberately carries the full response body (the TUI shows it), so
//! the error type is larger than clippy's default threshold.
#![allow(clippy::result_large_err)]
//!
//! - [`catalog`] parses the embedded service models.
//! - [`client`] signs and sends AWS API requests to one emulator endpoint.
//! - [`services`] turns list/describe calls into browsable tables.
//! - [`config`] discovers the endpoint, credentials and certificate files.
//! - [`supervisor`] starts, stops and tails a local emulator process.

pub mod catalog;
pub mod client;
pub mod config;
pub mod generic;
pub mod services;
pub mod sigv4;
pub mod supervisor;
pub mod xml;

pub use catalog::{catalog, Catalog, ServiceMeta};
pub use client::{ApiError, ApiResponse, EmulatorClient};
pub use config::{discover, ConnectionConfig, CredentialSource, EmulatorPaths};
pub use services::{ViewId, CURATED};
pub use supervisor::Supervisor;
