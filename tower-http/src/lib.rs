#![doc(html_root_url = "https://docs.rs/tower-http/0.1.0")]
#![allow(elided_lifetimes_in_paths, clippy::type_complexity)]
#![warn(
    clippy::all,
    clippy::dbg_macro,
    clippy::todo,
    clippy::empty_enum,
    clippy::enum_glob_use,
    clippy::pub_enum_variant_names,
    clippy::mem_forget,
    clippy::unused_self,
    clippy::filter_map_next,
    clippy::needless_continue,
    clippy::needless_borrow,
    clippy::match_wildcard_for_single_variants,
    clippy::if_let_mutex,
    clippy::mismatched_target_os,
    clippy::await_holding_lock,
    clippy::match_on_vec_items,
    clippy::imprecise_flops,
    clippy::suboptimal_flops,
    clippy::lossy_float_literal,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::fn_params_excessive_bools,
    clippy::exit,
    clippy::inefficient_to_string,
    clippy::linkedlist,
    clippy::macro_use_imports,
    clippy::option_option,
    clippy::verbose_file_reads,
    clippy::unnested_or_patterns,
    rust_2018_idioms,
    future_incompatible,
    nonstandard_style
)]
#![deny(unreachable_pub, broken_intra_doc_links, private_in_public)]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(test, allow(clippy::float_cmp))]

use http::Response;

#[cfg(feature = "trace")]
#[cfg_attr(docsrs, doc(cfg(feature = "trace")))]
pub mod trace;

#[derive(Debug, Clone, Copy)]
pub enum LatencyUnit {
    Millis,
    Nanos,
}

/// You might not always want to log the literal HTTP status. gRPC for example has its own status
/// and always uses `200 OK` even for errors.
// TODO(david): can we come up with a better name for this?
pub trait GetTraceStatus<T, E> {
    fn trace_status(&self, result: &Result<T, E>) -> TraceStatus;
}

// TODO(david): can we come up with a better name for this?
pub enum TraceStatus {
    Status(u16),
    Error,
}

#[derive(Copy, Clone)]
pub struct GetTraceStatusFromHttpStatus(());

impl<B, E> GetTraceStatus<Response<B>, E> for GetTraceStatusFromHttpStatus {
    fn trace_status(&self, result: &Result<Response<B>, E>) -> TraceStatus {
        match result {
            Ok(res) => TraceStatus::Status(res.status().as_u16()),
            Err(_) => TraceStatus::Error,
        }
    }
}

impl<T, E, F> GetTraceStatus<T, E> for F
where
    F: Fn(&Result<T, E>) -> TraceStatus,
{
    fn trace_status(&self, result: &Result<T, E>) -> TraceStatus {
        self(result)
    }
}
