//! The per-request context: where the tool's files are, and which secrets the request touched.

use crate::error::Redactor;
use crate::home::Home;

#[derive(Debug)]
pub struct Ctx {
    pub home: Home,
    pub redactor: Redactor,
}

impl Ctx {
    pub fn new(home: Home) -> Self {
        Self { home, redactor: Redactor::default() }
    }
}
