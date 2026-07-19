//! Offline tldr page parsing, cache discovery, and explicit update operations.

mod cache;
mod parser;

pub use cache::{
    HostPlatform, TldrCacheError, get_system_tldr_cache_dirs, get_tldr_cache_dir,
    get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs, normalize_tldr_topic,
    read_cached_tldr_page,
};
pub use parser::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};
