//! Named workspace receipt drivers.
//!
//! Each receipt is a scripted, headed run that asserts one product claim.
//! `routing` owns the step machine the others advance through.

mod a11y;
mod chrome;
mod reader;
mod routing;
