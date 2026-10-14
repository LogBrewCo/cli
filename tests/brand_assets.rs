//! Generate and verify CLI brand derivatives from the approved SDK artwork.

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// Fallible brand-tool operation.
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
/// Approved SDK app-icon bytes.
const APP_DIGEST: &str = "4547f1d32a87f90177bd430b70b4471e2aa5aceb99181c039e8233acbc561152";
/// Approved transparent inline-logo bytes.
const INLINE_DIGEST: &str = "a98dd05599862a0ba52c065c83fc60d386d04fa8b1efe46d11b0b6d4b5c4c8bc";
/// Sole generated repository image.
const ICON: &str = "wix/Product.ico";
/// Maximum size of each source, manifest, README, or installer icon.
const MAX_BYTES: u64 = 1_048_576;

/// Reject non-files, symlinks, and input growth beyond the read limit.
fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("brand input must be a bounded regular file".into());
    }
    let mut content = Vec::new();
    let _size = std::fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut content)?;
    if u64::try_from(content.len())? > MAX_BYTES {
        return Err("brand input exceeded its limit".into());
    }
    Ok(content)
}

/// Encode the content digest as lowercase hexadecimal.
fn digest(content: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(content)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

/// Accept only the pinned 256px RGB PNG, including its exact header and CRC.
fn build_ico(png: &[u8]) -> Result<Vec<u8>> {
    if digest(png) != APP_DIGEST {
        return Err("source is not the exact canonical SDK app icon".into());
    }
    let mut ico = vec![0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 32, 0];
    ico.extend(u32::try_from(png.len())?.to_le_bytes());
    ico.extend(22_u32.to_le_bytes());
    ico.extend_from_slice(png);
    Ok(ico)
}

/// Construct one complete canonical asset description.
fn asset(path: &str, sha256: &str, size: u16, color: &str, presentation: &str) -> Value {
    json!({
        "path": path,
        "sha256": sha256,
        "width": size,
        "height": size,
        "colorType": color,
        "presentation": presentation
    })
}

/// Exact object equality rejects missing, added, mistyped, or changed fields.
fn check_manifest(manifest: &Value, ico: &[u8]) -> Result<String> {
    let revision = manifest
        .pointer("/canonicalSource/revision")
        .and_then(Value::as_str)
        .ok_or("missing canonical revision")?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("canonical revision must be an exact commit".into());
    }
    let inline_path = "assets/brand/logbrew-logo-transparent-512.png";
    let expected = json!({
        "schemaVersion": 1,
        "canonicalSource": {
            "repository": "https://github.com/LogBrewCo/sdk",
            "revision": revision,
            "appIcon": asset("assets/brand/app-icon-256.png", APP_DIGEST, 256, "rgb", "espresso_app_icon"),
            "inlineLogo": asset(inline_path, INLINE_DIGEST, 512, "rgba", "transparent_inline")
        },
        "outputs": [{
            "path": ICON,
            "sha256": digest(ico),
            "kind": "ico_png",
            "width": 256,
            "height": 256,
            "presentation": "espresso_app_icon"
        }]
    });
    if *manifest != expected {
        return Err("brand manifest contract drifted".into());
    }
    Ok(format!(
        "https://raw.githubusercontent.com/LogBrewCo/sdk/{revision}/{inline_path}"
    ))
}

/// Bound repository traversal and reject every additional image derivative.
fn image_inventory(root: &Path) -> Result<std::collections::BTreeSet<PathBuf>> {
    let mut pending = vec![root.to_owned()];
    let mut images = std::collections::BTreeSet::new();
    let mut count = 0_u32;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            if count > 100_000 {
                return Err("brand inventory exceeded its limit".into());
            }
            if matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target")
            ) {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| {
                    ["ico", "icns", "jpeg", "jpg", "png", "svg"]
                        .contains(&extension.to_ascii_lowercase().as_str())
                })
            {
                let _inserted = images.insert(path.strip_prefix(root)?.to_owned());
            }
        }
    }
    Ok(images)
}

/// Verify provenance, exact pixels, output reproduction, and inline usage.
fn check_repository(root: &Path) -> Result {
    let manifest = serde_json::from_slice(&read(&root.join("wix/brand-assets.json"))?)?;
    let ico = read(&root.join(ICON))?;
    let png = ico.get(22..).ok_or("truncated installer icon")?;
    if build_ico(png)? != ico {
        return Err("installer icon directory or payload drifted".into());
    }
    let url = check_manifest(&manifest, &ico)?;
    let readme = String::from_utf8(read(&root.join("README.md"))?)?;
    let urls = readme
        .split("<img")
        .skip(1)
        .filter(|tail| tail.starts_with(char::is_whitespace))
        .filter_map(|tail| {
            tail.trim_start()
                .strip_prefix("src=\"")?
                .split_once('"')
                .map(|(url, _)| url)
        })
        .collect::<Vec<_>>();
    if urls != [url] || image_inventory(root)? != [PathBuf::from(ICON)].into() {
        return Err("README logo or local image inventory drifted".into());
    }
    Ok(())
}

/// Check the repository or atomically regenerate its approved installer icon.
fn run(root: &Path, args: &[std::ffi::OsString]) -> Result<String> {
    match args {
        [mode] if mode == "--check" => {
            check_repository(root)?;
            Ok("CLI brand assets ok\n".into())
        }
        [mode, source, path] if mode == "--write" && source == "--source" => {
            let ico = build_ico(&read(Path::new(path))?)?;
            let manifest = serde_json::from_slice(&read(&root.join("wix/brand-assets.json"))?)?;
            let _url = check_manifest(&manifest, &ico)?;
            let mut output = atomic_write_file::AtomicWriteFile::open(root.join(ICON))?;
            output.write_all(&ico)?;
            output.commit()?;
            Ok(format!(
                "generated wix/Product.ico sha256={}\n",
                digest(&ico)
            ))
        }
        _ => Err("use --check or --write --source <canonical-png>".into()),
    }
}

/// Keep filesystem paths and source details out of failure output.
fn main() -> std::process::ExitCode {
    match run(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &std::env::args_os().skip(1).collect::<Vec<_>>(),
    ) {
        Ok(message) if std::io::stdout().write_all(message.as_bytes()).is_ok() => {
            std::process::ExitCode::SUCCESS
        }
        Ok(_) | Err(_) => {
            let _written = std::io::stderr().write_all(b"CLI brand asset check failed.\n");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_exact_and_failed_inputs_preserve_the_output() -> Result {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = std::env::temp_dir().join(format!("logbrew-brand-{}", getrandom::u64()?));
        std::fs::create_dir(&root)?;
        let result = (|| -> Result {
            for directory in ["wix", "target"] {
                std::fs::create_dir(root.join(directory))?;
            }
            for name in ["README.md", "wix/brand-assets.json"] {
                std::fs::write(root.join(name), read(&repository.join(name))?)?;
            }
            let ico = read(&repository.join(ICON))?;
            let source = root.join("target/source.png");
            std::fs::write(&source, ico.get(22..).ok_or("missing PNG")?)?;
            let args = [
                "--write".into(),
                "--source".into(),
                source.clone().into_os_string(),
            ];
            assert_eq!(
                run(&root, &args)?,
                format!("generated wix/Product.ico sha256={}\n", digest(&ico)),
                "wrong generation receipt"
            );
            assert_eq!(read(&root.join(ICON))?, ico, "generated bytes drifted");
            assert_eq!(
                run(&root, &["--check".into()])?,
                "CLI brand assets ok\n",
                "check failed"
            );
            std::fs::write(&source, b"invalid source")?;
            assert!(run(&root, &args).is_err(), "invalid source accepted");
            assert_eq!(read(&root.join(ICON))?, ico, "failed write changed icon");
            std::fs::File::create(&source)?.set_len(MAX_BYTES + 1)?;
            assert!(read(&source).is_err(), "oversized source accepted");
            assert!(read(&root).is_err(), "directory accepted as source");
            std::fs::write(root.join("extra.PNG"), b"extra image")?;
            assert!(check_repository(&root).is_err(), "extra image accepted");
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn repository_and_exact_installer_roundtrip() -> Result {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        check_repository(root)?;
        let ico = read(&root.join(ICON))?;
        assert_eq!(
            build_ico(ico.get(22..).ok_or("missing PNG")?)?,
            ico,
            "ICO changed"
        );
        Ok(())
    }

    #[test]
    fn malformed_images_and_manifest_changes_fail_closed() -> Result {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let ico = read(&root.join(ICON))?;
        let png = ico.get(22..).ok_or("missing PNG")?;
        for index in [0, 8, 12, 16, 20, 24, 25, 26, 27, 28, 29, png.len() - 1] {
            let mut changed = png.to_vec();
            changed[index] ^= 1;
            assert!(build_ico(&changed).is_err(), "corrupt PNG accepted");
        }
        for length in [0, 8, 32, png.len() - 1] {
            assert!(build_ico(&png[..length]).is_err(), "truncated PNG accepted");
        }
        let manifest: Value = serde_json::from_slice(&read(&root.join("wix/brand-assets.json"))?)?;
        for pointer in [
            "/schemaVersion",
            "/canonicalSource/repository",
            "/canonicalSource/revision",
            "/canonicalSource/appIcon/sha256",
            "/canonicalSource/inlineLogo/presentation",
            "/outputs/0/path",
        ] {
            let mut changed = manifest.clone();
            *changed
                .pointer_mut(pointer)
                .ok_or("missing fixture field")? = json!("invalid");
            assert!(
                check_manifest(&changed, &ico).is_err(),
                "manifest drift accepted"
            );
        }
        Ok(())
    }
}
