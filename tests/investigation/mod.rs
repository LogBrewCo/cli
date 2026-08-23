//! Public CLI investigation contracts.

mod action_cursor_pagination;
mod api_rendering;
mod explain_contracts;
mod issue_cursor_pagination;
mod issue_investigation;
mod log_cursor_pagination;
#[path = "../integration/mock_http.rs"]
mod mock_http;
mod project_create;
mod span_investigation;
#[expect(
    dead_code,
    reason = "shared support is compiled by focused test targets"
)]
#[path = "../integration/support.rs"]
mod support;

pub(crate) use mock_http::{Mock, MockServer, Request, ResponseTemplate, retry_then};
pub(crate) use support::*;

pub(crate) mod matchers {
    pub(crate) use super::mock_http::matchers::*;
}
