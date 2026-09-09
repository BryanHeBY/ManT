//! Original compressed-tar fixtures exercise both container and transport EOF.

use std::io::{Cursor, Read, Write};

use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};

use super::{assert_extracts, extract_archive, extract_tar_with_budget, fs, tar_bytes, temp};

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("write gzip input");
    encoder.finish().expect("finish gzip fixture")
}

fn zstd(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).expect("zstd encoder");
    encoder
        .include_checksum(true)
        .expect("enable frame checksum");
    encoder.write_all(bytes).expect("write zstd input");
    encoder.finish().expect("finish zstd fixture")
}

fn assert_rejects(label: &str, bytes: &[u8]) {
    let root = temp(label);
    fs::create_dir_all(&root).expect("create fixture directory");
    let archive = root.join("download");
    fs::write(&archive, bytes).expect("write fixture archive");
    let result = extract_archive(&archive, &root.join("staging"));
    fs::remove_dir_all(&root).expect("remove fixture directory");
    assert!(
        result.is_err(),
        "{label}: invalid transport/tar tail must not succeed"
    );
}

#[test]
fn compressed_tar_validates_checksum_and_truncation_after_tar_end() {
    let tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    for (format, encoded, footer_length) in [("gzip", gzip(&tar), 8), ("zstd", zstd(&tar), 4)] {
        // Both fixtures have a valid document and tar end marker. Only the
        // outer compression footer is corrupted, so stopping at tar EOF is
        // insufficient even if every yielded file was read successfully.
        let mut corrupt = encoded.clone();
        let checksum = corrupt.len() - footer_length;
        corrupt[checksum] ^= 0x80;
        assert_rejects(&format!("{format}-bad-footer"), &corrupt);
        assert_rejects(
            &format!("{format}-truncated-footer"),
            &encoded[..encoded.len() - 1],
        );

        // A valid first member/frame can end after the whole tar archive.
        // A following padding member still must be validated completely.
        let padding = match format {
            "gzip" => gzip(&[0; 1_024]),
            _ => zstd(&[0; 1_024]),
        };
        let mut corrupt_padding = padding.clone();
        let checksum = corrupt_padding.len() - footer_length;
        corrupt_padding[checksum] ^= 0x80;
        let mut corrupt_tail = encoded.clone();
        corrupt_tail.extend(corrupt_padding);
        assert_rejects(&format!("{format}-bad-padding-member"), &corrupt_tail);

        let mut truncated_tail = encoded;
        truncated_tail.extend(&padding[..padding.len() - 1]);
        assert_rejects(
            &format!("{format}-truncated-padding-member"),
            &truncated_tail,
        );
    }
}

#[test]
fn compressed_tar_accepts_members_frames_and_zero_padding() {
    let tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    let split = tar.len() / 2;
    for (format, encode) in [("gzip", gzip as fn(&[u8]) -> Vec<u8>), ("zstd", zstd)] {
        let mut members = encode(&tar[..split]);
        members.extend(encode(&tar[split..]));
        // A compression boundary is not a tar boundary: both frames together
        // contain one complete logical archive.
        assert_extracts(&format!("{format}-split-members"), &members);

        let mut padded = encode(&tar);
        padded.extend(encode(&[0; 1_024]));
        assert_extracts(&format!("{format}-padding-member"), &padded);
    }
}

#[test]
fn tar_rejects_nonzero_tail_and_concatenated_archives() {
    let tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    for (label, tail) in [
        ("nonzero", b"unvalidated trailing data".to_vec()),
        ("second-tar", tar_bytes(&[("docs/other.md", b"# other")])),
    ] {
        let mut plain = tar.clone();
        plain.extend(&tail);
        assert_rejects(&format!("tar-tail-{label}"), &plain);
        for (format, encode) in [("gzip", gzip as fn(&[u8]) -> Vec<u8>), ("zstd", zstd)] {
            let mut members = encode(&tar);
            members.extend(encode(&tail));
            assert_rejects(&format!("{format}-tail-{label}"), &members);
        }
    }
}

#[test]
fn compressed_tar_rejects_uncompressed_transport_garbage() {
    let tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    for (format, mut encoded) in [("gzip", gzip(&tar)), ("zstd", zstd(&tar))] {
        // This suffix is not encoded as another member/frame. Reaching one
        // valid compression footer must not silently ignore trailing bytes.
        encoded.extend(b"not a gzip member or a zstd frame");
        assert_rejects(&format!("{format}-raw-transport-garbage"), &encoded);
    }
}

#[test]
fn exact_expanded_budget_does_not_skip_compression_checksums() {
    let tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    let budget = u64::try_from(tar.len()).expect("fixture length fits budget");
    for (format, mut bytes, footer_length) in [("gzip", gzip(&tar), 8), ("zstd", zstd(&tar), 4)] {
        let checksum = bytes.len() - footer_length;
        bytes[checksum] ^= 0x80;
        let root = temp(&format!("{format}-checksum-at-exact-budget"));
        fs::create_dir_all(&root).expect("create fixture directory");
        let reader: Box<dyn Read> = match format {
            "gzip" => Box::new(MultiGzDecoder::new(Cursor::new(&bytes))),
            _ => Box::new(zstd::stream::read::Decoder::new(Cursor::new(&bytes)).unwrap()),
        };
        let result = extract_tar_with_budget(reader, &root, budget);
        fs::remove_dir_all(&root).expect("remove fixture directory");
        let error = result.expect_err("checksum validation must occur even at exact byte budget");
        assert!(
            error.contains("could not finish tar archive stream"),
            "{format}: {error}"
        );
        assert!(
            !error.contains("decompressed stream exceeds"),
            "{format}: {error}"
        );
    }
}

#[test]
fn tar_end_padding_and_decoder_eof_still_obey_the_stream_budget() {
    let mut tar = tar_bytes(&[("docs/tool.md", b"# tool")]);
    tar.extend([0; 1_024]);
    let limit = u64::try_from(tar.len()).expect("fixture length fits budget");
    for (format, bytes) in [
        ("plain", tar.clone()),
        ("gzip", gzip(&tar)),
        ("zstd", zstd(&tar)),
    ] {
        for (case, budget, succeeds) in [("exact", limit, true), ("short", limit - 1, false)] {
            let root = temp(&format!("{format}-tar-eof-budget-{case}"));
            fs::create_dir_all(&root).expect("create fixture directory");
            let reader: Box<dyn Read> = match format {
                "gzip" => Box::new(MultiGzDecoder::new(Cursor::new(&bytes))),
                "zstd" => Box::new(zstd::stream::read::Decoder::new(Cursor::new(&bytes)).unwrap()),
                _ => Box::new(Cursor::new(&bytes)),
            };
            let result = extract_tar_with_budget(reader, &root, budget);
            fs::remove_dir_all(&root).expect("remove fixture directory");
            if succeeds {
                result.expect("exact budget must permit EOF and transport validation");
            } else {
                let error = result.expect_err("zero padding must consume decompressed budget");
                assert!(error.contains("decompressed stream exceeds"), "{error}");
            }
        }
    }
}
