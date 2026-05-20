//! `nami init`: guided first-run setup.
//!
//! Writes a minimal nami config file at the same path the region
//! resolver reads from ([`nami_region::config_path`]), with the chosen
//! `region` preset and a commented-out example profile so the schema is
//! immediately visible. Refuses to overwrite an existing file unless
//! `--force`; `--dry-run` prints the content without writing.
//!
//! After writing, `nami init` prints a brief snapshot of the remaining
//! preconditions (eGRID factors, `EIA_API_KEY`, per-region cache) with
//! concrete next-step commands. It does **not** itself run `nami
//! refresh` — surprise network calls and surprise files are exactly the
//! kind of automation users dislike on first contact.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

use nami_carbon_eia::{DEFAULT_CACHE_PATH, DEFAULT_EGRID_PATH, HistoricalCache};
use nami_core::Region;

use crate::InitArgs;

pub fn run(args: InitArgs) -> Result<()> {
    let path = args
        .config
        .clone()
        .or_else(nami_region::config_path)
        .ok_or_else(|| {
            anyhow!(
                "could not determine a config path; pass --config <path> or \
                 set $NAMI_CONFIG / $HOME"
            )
        })?;

    let content = render_config(args.region);

    if args.dry_run {
        println!("# would write to: {}", path.display());
        print!("{content}");
        return Ok(());
    }

    if path.exists() && !args.force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite, or edit it directly",
            path.display()
        ));
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
    }
    // Atomic write: temp file + rename, so a crash mid-write can't leave
    // a half-written config in place.
    let mut tmp_name = path
        .file_name()
        .ok_or_else(|| anyhow!("invalid config path {}", path.display()))?
        .to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, &content)
        .with_context(|| format!("writing temp file {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;

    println!("Wrote nami config to {}", path.display());
    print_post_init_snapshot(args.region);
    Ok(())
}

/// Render the config file content for a given default region. Pure, so
/// the exact bytes are unit-testable.
pub(crate) fn render_config(region: Region) -> String {
    format!(
        "# nami configuration.\n\
         #\n\
         # This file holds your default region and any named profiles.\n\
         # `nami preview` and `nami run` read it when --region or\n\
         # --duration/--deadline aren't given on the CLI.\n\
         #\n\
         # Region resolution order:\n\
         #   --region flag > --profile's region > NAMI_REGION env > this file's `region`.\n\
         # Anything passed on the CLI overrides a profile's value.\n\
         #\n\
         # Supported regions: CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP.\n\
         \n\
         region = \"{region}\"\n\
         \n\
         # Example profile (uncomment to use):\n\
         #\n\
         # [profiles.nightly]\n\
         # region   = \"{region}\"          # optional: override the default above\n\
         # duration = \"2h\"            # expected job duration\n\
         # within   = \"8h\"            # deadline = now + 8h at invocation\n\
         # command  = [\"cargo\", \"test\", \"--workspace\"]\n\
         #\n\
         # Or with an absolute deadline (alt. to `within`):\n\
         # deadline = \"2026-05-20T13:00:00Z\"\n"
    )
}

fn print_post_init_snapshot(region: Region) {
    let egrid_path = Path::new(DEFAULT_EGRID_PATH);
    let cache_path = Path::new(DEFAULT_CACHE_PATH);

    let egrid_ok = egrid_path.exists();
    let key_set = std::env::var("EIA_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let cache_has_region = matches!(
        HistoricalCache::load(cache_path),
        Ok(c) if !c.observations(region).is_empty()
    );

    println!();
    println!("Next steps:");
    if egrid_ok {
        println!("  ok    eGRID factor table present");
    } else {
        println!(
            "  todo  eGRID factor table missing at {}; rebuild with\n        \
             `cargo run -p nami-carbon-eia --features egrid-refresh --bin refresh-egrid`",
            egrid_path.display()
        );
    }
    if key_set {
        println!("  ok    EIA_API_KEY set");
    } else {
        println!(
            "  todo  EIA_API_KEY not set; needed for `nami refresh` (free key:\n        \
             https://www.eia.gov/opendata/register.php)"
        );
    }
    if cache_has_region {
        println!("  ok    {region} history cached");
    } else {
        println!(
            "  todo  no cached history for {region}; once EIA_API_KEY is set, run:\n        \
             nami refresh --region {region}"
        );
    }
    println!();
    println!("Run `nami status` any time for a full diagnostic.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use time::OffsetDateTime;

    fn unique_tmp(stem: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nami-init-test-{stem}-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ))
    }

    #[test]
    fn render_includes_region_and_example_profile() {
        let s = render_config(Region::Miso);
        assert!(s.contains("region = \"MISO\""));
        // The example profile is commented out, not active.
        assert!(s.contains("# [profiles.nightly]"));
        // The supported-region list is in the header for editability.
        assert!(s.contains("CAISO, ERCOT, MISO, PJM, NYISO, ISONE, SPP"));
    }

    #[test]
    fn render_is_valid_toml_and_resolves_via_region_crate() {
        let s = render_config(Region::Pjm);
        // The written content must parse cleanly with the same resolver
        // the rest of the CLI uses, so init produces a working config.
        let resolved = nami_region::resolve(
            None,
            None,
            Some((s.as_str(), PathBuf::from("/tmp/test.toml"))),
        )
        .unwrap();
        assert_eq!(resolved.region, Region::Pjm);
        assert!(matches!(
            resolved.source,
            nami_region::RegionSource::Config(_)
        ));
    }

    #[test]
    fn writes_atomic_and_round_trips() {
        let dir = unique_tmp("write");
        let path = dir.join("config.toml");
        // The directory doesn't exist yet — init should create it.
        let args = InitArgs {
            region: Region::Caiso,
            config: Some(path.clone()),
            force: false,
            dry_run: false,
        };
        run(args).expect("init should succeed");

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("region = \"CAISO\""));

        // No leftover .tmp from the atomic-write dance.
        assert!(!path.with_extension("toml.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_to_overwrite_without_force() {
        let dir = unique_tmp("refuse");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "region = \"MISO\"\n").unwrap();
        let args = InitArgs {
            region: Region::Ercot,
            config: Some(path.clone()),
            force: false,
            dry_run: false,
        };
        let err = run(args).unwrap_err().to_string();
        assert!(err.contains("already exists"));
        // The original file is untouched.
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "region = \"MISO\"\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_overwrites_existing_file() {
        let dir = unique_tmp("force");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "region = \"MISO\"\n").unwrap();
        let args = InitArgs {
            region: Region::Ercot,
            config: Some(path.clone()),
            force: true,
            dry_run: false,
        };
        run(args).expect("--force should succeed");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("region = \"ERCOT\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_does_not_write() {
        let dir = unique_tmp("dry");
        let path = dir.join("config.toml");
        let args = InitArgs {
            region: Region::Spp,
            config: Some(path.clone()),
            force: false,
            dry_run: true,
        };
        run(args).expect("dry-run should succeed");
        assert!(!path.exists());
    }
}
