//! Original compressed inputs exercise the loader before any partial IR exists.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use flate2::{Compression, write::GzEncoder};
use mant_ir::{Inline, visit::Visit};

use super::{MAX_MANUAL_BYTES, ManualBudget, load_manual_source_with_budget};
use crate::{ManualErrorKind, parse_manual_source};

const FIRST: &[u8] = b".TH COMPRESSED 1\n.SH FIRST\nTOKENA\n";
const SECOND: &[u8] = b".SH SECOND\nTOKENB\n";
static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
enum Encoding {
    Gzip,
    Zstd,
}

impl Encoding {
    fn extension(self) -> &'static str {
        match self {
            Self::Gzip => "gz",
            Self::Zstd => "zst",
        }
    }

    fn encode(self, source: &[u8]) -> Vec<u8> {
        match self {
            Self::Gzip => {
                let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
                encoder.write_all(source).expect("encode gzip source");
                encoder.finish().expect("finish gzip member")
            }
            Self::Zstd => {
                let mut encoder =
                    zstd::stream::write::Encoder::new(Vec::new(), 1).expect("create zstd encoder");
                encoder
                    .include_checksum(true)
                    .expect("enable frame checksum");
                encoder.write_all(source).expect("encode zstd source");
                encoder.finish().expect("finish zstd frame")
            }
        }
    }

    fn damage_checksum(self, encoded: &mut [u8]) {
        let footer_length = match self {
            Self::Gzip => 8,
            Self::Zstd => 4,
        };
        let checksum_start = encoded.len() - footer_length;
        encoded[checksum_start] ^= 1;
    }
}

struct Fixture {
    root: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new(encoding: Encoding) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "mant-manual-compression-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create private compression fixture");
        Self {
            path: root.join(format!("probe.1.{}", encoding.extension())),
            root,
        }
    }

    fn write(&self, bytes: &[u8]) {
        fs::write(&self.path, bytes).expect("write compressed manual");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Default)]
struct VisibleText(String);

impl<'ir> Visit<'ir> for VisibleText {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } => self.0.push_str(value),
            _ => mant_ir::visit::walk_inline(self, inline),
        }
    }
}

fn accepts_all_members(encoding: Encoding) {
    let fixture = Fixture::new(encoding);
    let mut encoded = encoding.encode(FIRST);
    // A valid empty member/frame is different from an absent compressed stream.
    encoded.extend(encoding.encode(b""));
    encoded.extend(encoding.encode(SECOND));
    fixture.write(&encoded);
    let document = parse_manual_source(&fixture.path).expect("decode every member before parsing");
    assert_eq!(document.meta.title.as_deref(), Some("COMPRESSED"));
    let mut visible = VisibleText::default();
    visible.visit_document(&document);
    assert!(visible.0.contains("TOKENA"));
    assert!(visible.0.contains("TOKENB"));
    assert_eq!(document.sections.len(), 2);
}

fn rejects_malformed_members_without_partial_ir(encoding: Encoding) {
    let fixture = Fixture::new(encoding);
    let good = encoding.encode(FIRST);
    let mut bad_header = good.clone();
    bad_header[0] ^= 1;
    let truncated_header = good[..3].to_vec();
    let mut truncated_footer = good.clone();
    truncated_footer.pop();
    let mut bad_checksum = good.clone();
    encoding.damage_checksum(&mut bad_checksum);
    let mut broken_second = good.clone();
    broken_second.extend(&truncated_footer);
    for (name, input) in [
        ("empty encoded input", Vec::new()),
        ("invalid header", bad_header),
        ("truncated header", truncated_header),
        ("truncated footer", truncated_footer),
        ("checksum mismatch", bad_checksum),
        ("valid member followed by incomplete member", broken_second),
    ] {
        fixture.write(&input);
        let error = parse_manual_source(&fixture.path)
            .expect_err("decompression failure must not expose an IR document");
        assert_eq!(
            error.kind(),
            ManualErrorKind::Decompression,
            "{name}: {error}"
        );
        assert_eq!(error.path(), fixture.path);
    }
}

fn expansion_limits_precede_ir_construction(encoding: Encoding) {
    let fixture = Fixture::new(encoding);
    let mut source = FIRST.to_vec();
    source.resize(4096, b'\n');
    let encoded = encoding.encode(&source);
    assert!(encoded.len() < 512, "fixture must exercise decoded budget");
    fixture.write(&encoded);
    let mut budget = ManualBudget::new(512);
    let error = load_manual_source_with_budget(&fixture.path, &mut budget)
        .err()
        .expect("high-ratio input exceeds the remaining decoded budget");
    assert_eq!(error.kind(), ManualErrorKind::Limit);
    assert_eq!(budget.decoded_remaining, 512);

    let mut sufficient = ManualBudget::new(4096);
    let loaded = load_manual_source_with_budget(&fixture.path, &mut sufficient)
        .expect("exact decoded budget accepts the complete stream");
    assert_eq!(loaded.source, source);
    assert_eq!(sufficient.decoded_remaining, 0);

    source.resize(usize::try_from(MAX_MANUAL_BYTES + 1).unwrap(), b'\n');
    let encoded = encoding.encode(&source);
    assert!(u64::try_from(encoded.len()).unwrap() < MAX_MANUAL_BYTES);
    fixture.write(&encoded);
    let error = parse_manual_source(&fixture.path)
        .expect_err("public loader must reject expansion rather than parse the valid prefix");
    assert_eq!(error.kind(), ManualErrorKind::Limit);
}

#[test]
fn gzip_reads_all_members_including_empty_members() {
    accepts_all_members(Encoding::Gzip);
}

#[test]
fn zstd_reads_all_frames_including_empty_frames() {
    accepts_all_members(Encoding::Zstd);
}

#[test]
fn gzip_rejects_malformed_members_before_producing_ir() {
    rejects_malformed_members_without_partial_ir(Encoding::Gzip);
}

#[test]
fn zstd_rejects_malformed_frames_before_producing_ir() {
    rejects_malformed_members_without_partial_ir(Encoding::Zstd);
}

#[test]
fn gzip_expansion_is_bounded_before_producing_ir() {
    expansion_limits_precede_ir_construction(Encoding::Gzip);
}

#[test]
fn zstd_expansion_is_bounded_before_producing_ir() {
    expansion_limits_precede_ir_construction(Encoding::Zstd);
}
