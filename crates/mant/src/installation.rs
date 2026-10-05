//! Private installer bridge. Never dispatched by MCP or ordinary document queries.

use mant_sources::settings::Settings;
use std::io::Write;

pub(crate) fn run(
    arguments: &[String],
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Option<u8> {
    let action = arguments.first()?.as_str();
    if !matches!(action, "--installer-paths" | "--installer-migrate") {
        return None;
    }
    let result = execute(action, &arguments[1..], output);
    Some(match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(diagnostics, "mant installer: {error}");
            1
        }
    })
}

fn execute(
    action: &str,
    arguments: &[String],
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = if action == "--installer-migrate" {
        let [root] = arguments else {
            return Err("--installer-migrate requires one legacy root".into());
        };
        #[cfg(feature = "update")]
        {
            mant_sources::migration::migrate_legacy_storage(std::path::Path::new(root))?
        }
        #[cfg(not(feature = "update"))]
        {
            let _ = root;
            return Err("installer migration requires the update feature".into());
        }
    } else {
        Settings::load()?
    };
    let config = settings.config_home()?;
    let data = settings.data_home()?;
    let cache = settings.cache_home()?;
    let _ = settings.manual_paths()?;
    if action == "--installer-paths" && !arguments.is_empty() {
        let [field] = arguments else {
            return Err("--installer-paths accepts at most one field".into());
        };
        let path = match field.as_str() {
            "config" => config,
            "data" => data.clone(),
            "cache" => cache,
            "documents" => data.join("documents"),
            _ => return Err("unknown installer path field".into()),
        };
        let value = path.to_str().ok_or("installer paths must be UTF-8")?;
        if value.chars().any(char::is_control) {
            return Err("installer paths contain control characters".into());
        }
        writeln!(output, "{value}")?;
    } else {
        serde_json::to_writer(
            &mut *output,
            &serde_json::json!({"config": config, "data": data, "cache": cache, "documents": data.join("documents")}),
        )?;
        writeln!(output)?;
    }
    Ok(())
}
