//! Bounded HTTP archive downloads with conditional-request support.

use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

use sha2::{Digest, Sha256};

use crate::archive::MAX_ARCHIVE_BYTES;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validators {
    fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

pub(crate) enum DownloadOutcome {
    NotModified,
    Downloaded {
        revision: String,
        validators: Validators,
    },
}

pub(crate) fn download_archive(
    url: &str,
    destination: &Path,
    validators: Option<&Validators>,
) -> Result<DownloadOutcome, String> {
    let agent = archive_agent(url)?;
    let mut request = agent
        .get(url)
        .header("Accept-Encoding", "identity")
        .header("User-Agent", concat!("mant/", env!("CARGO_PKG_VERSION")));
    let validators = validators.filter(|validators| !validators.is_empty());
    if let Some(validators) = validators {
        if let Some(etag) = &validators.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(last_modified) = &validators.last_modified {
            request = request.header("If-Modified-Since", last_modified);
        }
    }

    let mut response = request
        .call()
        .map_err(|error| format!("could not download archive: {error}"))?;
    let status = response.status().as_u16();
    if status == 304 && validators.is_some() {
        return Ok(DownloadOutcome::NotModified);
    }
    if status != 200 {
        return Err(format!("archive server returned HTTP {status}"));
    }
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
    {
        return Err(format!(
            "downloaded archive exceeds the {MAX_ARCHIVE_BYTES}-byte limit"
        ));
    }
    let response_validators = Validators {
        etag: response_header(&response, "etag"),
        last_modified: response_header(&response, "last-modified"),
    };

    let result = write_download(&mut response, destination, response_validators);
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn archive_agent(url: &str) -> Result<ureq::Agent, String> {
    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|error| format!("invalid archive URL: {error}"))?;
    let test_loopback_http =
        cfg!(test) && uri.scheme_str() == Some("http") && uri.host().is_some_and(is_loopback_host);
    if uri.scheme_str() != Some("https") && !test_loopback_http {
        return Err("archive URL must use HTTPS".to_owned());
    }
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .https_only(!test_loopback_http)
        .max_redirects(5)
        .max_redirects_will_error(true);
    if test_loopback_http {
        config = config.proxy(None);
    }
    Ok(config.build().into())
}

fn write_download(
    response: &mut ureq::http::Response<ureq::Body>,
    destination: &Path,
    validators: Validators,
) -> Result<DownloadOutcome, String> {
    let mut output = File::options()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create archive download: {error}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read archive response: {error}"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).expect("buffer length fits in u64"))
            .ok_or_else(|| "archive download size overflow".to_owned())?;
        if total > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "downloaded archive exceeds the {MAX_ARCHIVE_BYTES}-byte limit"
            ));
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("could not write archive download: {error}"))?;
        hasher.update(&buffer[..count]);
    }
    output
        .sync_all()
        .map_err(|error| format!("could not sync archive download: {error}"))?;
    let mut revision = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut revision, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(DownloadOutcome::Downloaded {
        revision,
        validators,
    })
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || matches!(host, "127.0.0.1" | "::1")
}

fn response_header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read as _, Write as _},
        net::{Shutdown, TcpListener},
        thread,
    };

    use super::{DownloadOutcome, Validators, download_archive};

    fn temp(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-download-{label}-{}", std::process::id()))
    }

    fn serve(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("read request");
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            stream.write_all(&response).expect("write response");
            stream.flush().expect("flush response");
            stream.shutdown(Shutdown::Write).expect("finish response");
            String::from_utf8(request).expect("UTF-8 request")
        });
        (format!("http://{address}/docs"), handle)
    }

    #[test]
    fn download_is_bounded_hashed_and_records_validators() {
        let body = b"archive";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"v1\"\r\nLast-Modified: Tue, 11 Aug 2026 00:00:00 GMT\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect();
        let (url, server) = serve(response);
        let destination = temp("download");
        let outcome = download_archive(&url, &destination, None).expect("download archive");
        let DownloadOutcome::Downloaded {
            revision,
            validators,
        } = outcome
        else {
            panic!("expected downloaded archive");
        };
        assert_eq!(revision.len(), 64);
        assert_eq!(validators.etag.as_deref(), Some("\"v1\""));
        assert_eq!(
            validators.last_modified.as_deref(),
            Some("Tue, 11 Aug 2026 00:00:00 GMT")
        );
        assert_eq!(fs::read(&destination).expect("read download"), body);
        let request = server.join().expect("join server").to_ascii_lowercase();
        assert!(request.contains("accept-encoding: identity"));
        fs::remove_file(destination).expect("remove download");
    }

    #[test]
    fn conditional_download_accepts_not_modified() {
        let (url, server) = serve(
            b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        );
        let destination = temp("not-modified");
        let outcome = download_archive(
            &url,
            &destination,
            Some(&Validators {
                etag: Some("\"v1\"".to_owned()),
                last_modified: Some("Tue, 11 Aug 2026 00:00:00 GMT".to_owned()),
            }),
        )
        .expect("conditional download");
        assert!(matches!(outcome, DownloadOutcome::NotModified));
        assert!(!destination.exists());
        let request = server.join().expect("join server").to_ascii_lowercase();
        assert!(request.contains("if-none-match: \"v1\""));
        assert!(request.contains("if-modified-since: tue, 11 aug 2026 00:00:00 gmt"));
    }
}
