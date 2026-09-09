//! Application tldr maintenance composed with read-only loader discovery.

#[cfg(feature = "tldr-update")]
mod update;

pub use mant_codec::{TldrPageLocation, TldrParseError, parse_tldr_command, parse_tldr_page};
pub use mant_loader::{
    HostPlatform, TldrCacheError, get_system_tldr_cache_dirs, get_tldr_cache_dir,
    get_tldr_languages, get_tldr_platforms, get_tldr_read_cache_dirs, normalize_tldr_topic,
    read_cached_tldr_page,
};
#[cfg(feature = "tldr-update")]
pub use update::{TldrUpdateError, update_tldr_cache};
