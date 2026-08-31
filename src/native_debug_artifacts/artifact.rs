//! Local native debug-object discovery and validation.

use crate::RuntimeError;
use object::read::macho::{MachOFatFile, MachOFatFile32, MachOFatFile64, MachOFile64};
use object::{
    Architecture, FileKind, Object as _, ObjectSection as _, SectionKind, SubArchitecture,
};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::io::{Read as _, Seek as _};
use std::path::{Path, PathBuf};

/// Maximum number of exact object identities accepted by one upload.
const MAX_ARTIFACTS: usize = 50;
/// Maximum bytes accepted for one uploaded thin debug object.
pub(super) const MAX_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;
/// Maximum bytes accepted for one source file before slice extraction.
const MAX_SOURCE_BYTES: usize = 512 * 1024 * 1024;
/// Fixed public resumable upload chunk size.
pub(super) const RESUMABLE_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// Maximum central-directory entries inspected in one bounded ZIP.
const MAX_ZIP_ENTRIES: usize = 2_000;
/// Maximum expansion ratio after a small fixed allowance.
const MAX_ZIP_EXPANSION_RATIO: u64 = 100;
/// Small files may expand from compact deflate streams without being ZIP bombs.
const ZIP_EXPANSION_ALLOWANCE: u64 = 1024 * 1024;

/// One supported architecture accepted by the public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NativeArchitecture {
    /// 32-bit ARM.
    Arm,
    /// Standard 64-bit ARM.
    Arm64,
    /// Pointer-authenticated Apple arm64e.
    Arm64E,
    /// Intel x86-64.
    X86_64,
    /// Intel x86.
    X86,
}

/// One native artifact family accepted by the hosted contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeArtifactType {
    /// Apple Mach-O debug object.
    AppleDsym,
    /// Android ELF debug object.
    AndroidElf,
}

impl NativeArtifactType {
    /// Returns the upload manifest discriminator.
    pub(super) const fn manifest(self) -> &'static str {
        match self {
            Self::AppleDsym => "apple_dsym_manifest",
            Self::AndroidElf => "android_elf_manifest",
        }
    }

    /// Returns the lookup response discriminator.
    pub(super) const fn artifact(self) -> &'static str {
        match self {
            Self::AppleDsym => "apple_dsym",
            Self::AndroidElf => "android_elf",
        }
    }
}

impl NativeArchitecture {
    /// Returns the canonical public API value.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Arm => "arm",
            Self::Arm64 => "arm64",
            Self::Arm64E => "arm64e",
            Self::X86_64 => "x86_64",
            Self::X86 => "x86",
        }
    }
}

/// One validated native debug payload and its exact public identity.
pub(super) struct Artifact {
    /// Canonical lowercase image UUID.
    pub(super) image_uuid: String,
    /// Canonical supported architecture.
    pub(super) architecture: NativeArchitecture,
    /// Validated native artifact family.
    pub(super) kind: NativeArtifactType,
    /// Lowercase SHA-256 of exactly the uploaded bytes.
    pub(super) sha256: String,
    /// Exact native debug bytes uploaded for this identity.
    pub(super) bytes: bytes::Bytes,
}

/// One immutable resumable chunk derived from validated artifact bytes.
pub(super) struct ArtifactChunk {
    /// Lowercase SHA-256 of the exact chunk bytes.
    pub(super) sha256: String,
    /// Cheap immutable slice of the parent artifact bytes.
    pub(super) bytes: bytes::Bytes,
}

impl Artifact {
    /// Returns the exact uploaded byte count.
    pub(super) fn byte_size(&self) -> u64 {
        u64::try_from(self.bytes.len()).unwrap_or(u64::MAX)
    }

    /// Splits validated bytes into fixed ordered SHA-256 chunks without copying payload data.
    pub(super) fn resumable_chunks(&self) -> Vec<ArtifactChunk> {
        (0..self.bytes.len())
            .step_by(RESUMABLE_CHUNK_BYTES)
            .map(|start| {
                let end = start
                    .saturating_add(RESUMABLE_CHUNK_BYTES)
                    .min(self.bytes.len());
                let bytes = self.bytes.slice(start..end);
                ArtifactChunk {
                    sha256: sha256_hex(bytes.as_ref()),
                    bytes,
                }
            })
            .collect()
    }
}

/// Enumerates one dSYM bundle, symbol ZIP, Mach-O, or ELF object.
pub(super) fn collect(path: &Path) -> Result<Vec<Artifact>, RuntimeError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid_artifact())?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_artifact());
    }
    let artifacts = if metadata.is_dir()
        && path.extension().and_then(std::ffi::OsStr::to_str) == Some("dSYM")
    {
        let dwarf = path.join("Contents/Resources/DWARF");
        collect_bundle(dwarf.as_path())?
    } else if metadata.is_file() {
        if has_zip_extension(path) {
            collect_zip(path)?
        } else {
            parse_debug_file(read_regular_file(path)?)?
        }
    } else {
        return Err(invalid_artifact());
    };
    finalize(artifacts)
}

/// Parses every regular debug object in a local dSYM bundle.
fn collect_bundle(root: &Path) -> Result<Vec<Artifact>, RuntimeError> {
    let files = collect_regular_files(root)?;
    let mut artifacts = Vec::new();
    for file in files {
        let bytes = read_regular_file(file.as_path())?;
        let mut parsed = parse_macho(bytes)?;
        if parsed.is_empty() {
            return Err(invalid_artifact());
        }
        artifacts.append(&mut parsed);
        if artifacts.len() > MAX_ARTIFACTS {
            return Err(invalid_artifact());
        }
    }
    Ok(artifacts)
}

/// Parses every exact dSYM debug object from one bounded ZIP.
fn collect_zip(path: &Path) -> Result<Vec<Artifact>, RuntimeError> {
    let (file, before) = open_regular_file(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| invalid_artifact())?;
    if archive.is_empty() || archive.len() > MAX_ZIP_ENTRIES {
        return Err(invalid_artifact());
    }

    let mut artifacts = Vec::new();
    let mut names = BTreeSet::new();
    let mut total_uncompressed = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| invalid_artifact())?;
        let name = validated_zip_name(&entry)?;
        if !names.insert(name.clone())
            || entry.encrypted()
            || entry.is_symlink()
            || !(entry.is_file() || entry.is_dir())
        {
            return Err(invalid_artifact());
        }
        let size = entry.size();
        total_uncompressed = total_uncompressed
            .checked_add(size)
            .filter(|total| *total <= u64::try_from(MAX_SOURCE_BYTES).unwrap_or(u64::MAX))
            .ok_or_else(invalid_artifact)?;
        if zip_expansion_is_unsafe(entry.compressed_size(), size) {
            return Err(invalid_artifact());
        }
        if entry.is_dir() {
            if size != 0 {
                return Err(invalid_artifact());
            }
            continue;
        }

        if is_debug_object(name.as_str()) {
            let declared_size = usize::try_from(size).map_err(|_| invalid_artifact())?;
            if declared_size == 0 || declared_size > MAX_SOURCE_BYTES {
                return Err(invalid_artifact());
            }
            let mut payload = Vec::with_capacity(declared_size);
            let read_limit = size.saturating_add(1);
            let read = (&mut entry)
                .take(read_limit)
                .read_to_end(&mut payload)
                .map_err(|_| invalid_artifact())?;
            if read != declared_size || payload.len() != declared_size {
                return Err(invalid_artifact());
            }
            let mut parsed = parse_debug_file(payload)?;
            if parsed.is_empty() {
                return Err(invalid_artifact());
            }
            artifacts.append(&mut parsed);
            if artifacts.len() > MAX_ARTIFACTS {
                return Err(invalid_artifact());
            }
        } else {
            let read_limit = size.saturating_add(1);
            let copied = std::io::copy(&mut (&mut entry).take(read_limit), &mut std::io::sink())
                .map_err(|_| invalid_artifact())?;
            if copied != size {
                return Err(invalid_artifact());
            }
        }
    }
    let mut file = archive.into_inner();
    file.rewind().map_err(|_| invalid_artifact())?;
    let after = file.metadata().map_err(|_| invalid_artifact())?;
    if !same_file(&before, &after) {
        return Err(invalid_artifact());
    }
    Ok(artifacts)
}

/// Sorts object identities and rejects duplicates or empty discovery.
fn finalize(mut artifacts: Vec<Artifact>) -> Result<Vec<Artifact>, RuntimeError> {
    artifacts.sort_by(|left, right| {
        (left.image_uuid.as_str(), left.architecture)
            .cmp(&(right.image_uuid.as_str(), right.architecture))
    });
    let mut identities = BTreeSet::new();
    for artifact in &artifacts {
        if !identities.insert((artifact.image_uuid.clone(), artifact.architecture)) {
            return Err(invalid_artifact());
        }
    }
    if artifacts.is_empty()
        || artifacts.len() > MAX_ARTIFACTS
        || artifacts
            .iter()
            .any(|artifact| artifact.kind != artifacts[0].kind)
    {
        return Err(invalid_artifact());
    }
    Ok(artifacts)
}

/// Requires the discovered UUID set to match the user-supplied release identities exactly.
pub(super) fn validate_expected_uuids(
    artifacts: &[Artifact],
    expected: &[String],
) -> Result<(), RuntimeError> {
    if expected.is_empty() {
        return Ok(());
    }
    let discovered = artifacts
        .iter()
        .map(|artifact| artifact.image_uuid.as_str())
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if discovered != expected {
        return Err(invalid_artifact());
    }
    Ok(())
}

/// Restricts archive uploads to explicit ZIP inputs.
fn has_zip_extension(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

/// Validates one unambiguous portable ZIP entry name.
fn validated_zip_name<R: std::io::Read>(
    entry: &zip::read::ZipFile<'_, R>,
) -> Result<String, RuntimeError> {
    let raw = std::str::from_utf8(entry.name_raw()).map_err(|_| invalid_artifact())?;
    if raw != entry.name()
        || raw.is_empty()
        || raw.starts_with('/')
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
        || entry.enclosed_name().is_none()
    {
        return Err(invalid_artifact());
    }
    let name = raw.strip_suffix('/').unwrap_or(raw);
    if name.is_empty()
        || name.split('/').any(|component| {
            component.is_empty() || matches!(component, "." | "..") || component.contains(':')
        })
    {
        return Err(invalid_artifact());
    }
    Ok(name.to_owned())
}

/// Detects exact dSYM objects or Android shared objects in one symbol ZIP.
fn is_debug_object(name: &str) -> bool {
    let components = name.split('/').collect::<Vec<_>>();
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("so"))
        || components.windows(5).any(|window| {
            window[0].ends_with(".dSYM")
                && window[1] == "Contents"
                && window[2] == "Resources"
                && window[3] == "DWARF"
                && !window[4].is_empty()
        })
}

/// Rejects compressed entries with an excessive declared expansion ratio.
const fn zip_expansion_is_unsafe(compressed_size: u64, size: u64) -> bool {
    size > compressed_size
        .saturating_mul(MAX_ZIP_EXPANSION_RATIO)
        .saturating_add(ZIP_EXPANSION_ALLOWANCE)
}

/// Recursively collects only regular files beneath a dSYM DWARF directory.
fn collect_regular_files(root: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| invalid_artifact())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_artifact());
    }
    let mut files = Vec::new();
    collect_directory(root, &mut files)?;
    files.sort();
    Ok(files)
}

/// Traverses one directory level while rejecting links and special files.
fn collect_directory(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), RuntimeError> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|_| invalid_artifact())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid_artifact())?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(path.as_path()).map_err(|_| invalid_artifact())?;
        if metadata.file_type().is_symlink() {
            return Err(invalid_artifact());
        }
        if metadata.is_dir() {
            collect_directory(path.as_path(), files)?;
        } else if metadata.is_file() {
            files.push(path);
        } else {
            return Err(invalid_artifact());
        }
    }
    Ok(())
}

/// Opens and reads one stable regular file with a hard upper bound.
fn read_regular_file(path: &Path) -> Result<Vec<u8>, RuntimeError> {
    let (mut file, before) = open_regular_file(path)?;
    let mut bytes = Vec::new();
    let read_limit = u64::try_from(MAX_SOURCE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let bytes_read = (&mut file)
        .take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_artifact())?;
    let after = file.metadata().map_err(|_| invalid_artifact())?;
    if bytes_read != bytes.len()
        || bytes.len() > MAX_SOURCE_BYTES
        || u64::try_from(bytes.len()).ok() != Some(before.len())
        || !same_file(&before, &after)
    {
        return Err(invalid_artifact());
    }
    Ok(bytes)
}

/// Opens one stable bounded regular source without following a pre-existing link.
fn open_regular_file(path: &Path) -> Result<(std::fs::File, std::fs::Metadata), RuntimeError> {
    let before = std::fs::symlink_metadata(path).map_err(|_| invalid_artifact())?;
    if before.file_type().is_symlink()
        || !before.is_file()
        || usize::try_from(before.len()).map_or(true, |length| length > MAX_SOURCE_BYTES)
    {
        return Err(invalid_artifact());
    }
    let file = std::fs::File::open(path).map_err(|_| invalid_artifact())?;
    let opened = file.metadata().map_err(|_| invalid_artifact())?;
    if !opened.is_file() || !same_file(&before, &opened) {
        return Err(invalid_artifact());
    }
    Ok((file, before))
}

/// Compares stable file identity across reads on Unix.
#[cfg(unix)]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    left.dev() == right.dev() && left.ino() == right.ino() && left.len() == right.len()
}

/// Compares the available stable metadata on non-Unix platforms.
#[cfg(not(unix))]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.is_file() == right.is_file()
}

/// Parses one thin or universal Mach-O into exact supported thin slices.
fn parse_macho(bytes: Vec<u8>) -> Result<Vec<Artifact>, RuntimeError> {
    let payload = bytes::Bytes::from(bytes);
    match FileKind::parse(payload.as_ref()).map_err(|_| invalid_artifact())? {
        FileKind::MachO64 => parse_supported_slice(payload)
            .map(|artifact| artifact.map_or_else(Vec::new, |artifact| vec![artifact])),
        FileKind::MachOFat32 => {
            let fat = MachOFatFile32::parse(payload.as_ref()).map_err(|_| invalid_artifact())?;
            parse_fat(&fat, &payload)
        }
        FileKind::MachOFat64 => {
            let fat = MachOFatFile64::parse(payload.as_ref()).map_err(|_| invalid_artifact())?;
            parse_fat(&fat, &payload)
        }
        FileKind::DyldCache | FileKind::MachO32 | _ => Err(invalid_artifact()),
    }
}

/// Parses one locally validated Mach-O or Android ELF debug file.
fn parse_debug_file(bytes: Vec<u8>) -> Result<Vec<Artifact>, RuntimeError> {
    let kind = FileKind::parse(bytes.as_slice()).map_err(|_| invalid_artifact())?;
    if matches!(kind, FileKind::Elf32 | FileKind::Elf64) {
        parse_elf(bytes).map(|artifact| vec![artifact])
    } else {
        parse_macho(bytes)
    }
}

/// Parses one complete ELF object and derives its UUID from GNU Build ID bytes.
fn parse_elf(bytes: Vec<u8>) -> Result<Artifact, RuntimeError> {
    let payload = bytes::Bytes::from(bytes);
    if !artifact_size_allowed(payload.len()) {
        return Err(invalid_artifact());
    }
    let file = object::File::parse(payload.as_ref()).map_err(|_| invalid_artifact())?;
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "all unsupported and future ELF architectures fail closed"
    )]
    let architecture = match file.architecture() {
        Architecture::Arm => NativeArchitecture::Arm,
        Architecture::Aarch64 => NativeArchitecture::Arm64,
        Architecture::I386 => NativeArchitecture::X86,
        Architecture::X86_64 => NativeArchitecture::X86_64,
        _ => return Err(invalid_artifact()),
    };
    let has_debug_info = file.sections().any(|section| {
        section.name().ok() == Some(".debug_info")
            && section.data().is_ok_and(|data| !data.is_empty())
    });
    if !has_debug_info {
        return Err(invalid_artifact());
    }
    let build_id = file
        .build_id()
        .map_err(|_| invalid_artifact())?
        .filter(|value| (16..=32).contains(&value.len()) && value.iter().any(|byte| *byte != 0))
        .ok_or_else(invalid_artifact)?;
    let uuid = <[u8; 16]>::try_from(&build_id[..16]).map_err(|_| invalid_artifact())?;
    Ok(Artifact {
        image_uuid: format_uuid(uuid),
        architecture,
        kind: NativeArtifactType::AndroidElf,
        sha256: sha256_hex(payload.as_ref()),
        bytes: payload,
    })
}

/// Parses every supported slice from one universal Mach-O.
fn parse_fat<Fat: object::read::macho::FatArch>(
    fat: &MachOFatFile<'_, Fat>,
    bytes: &bytes::Bytes,
) -> Result<Vec<Artifact>, RuntimeError> {
    let mut artifacts = Vec::new();
    for arch in fat.arches() {
        let (offset, size) = arch.file_range();
        let start = usize::try_from(offset).map_err(|_| invalid_artifact())?;
        let size = usize::try_from(size).map_err(|_| invalid_artifact())?;
        let end = start.checked_add(size).ok_or_else(invalid_artifact)?;
        if end > bytes.len() {
            return Err(invalid_artifact());
        }
        if let Some(artifact) = parse_supported_slice(bytes.slice(start..end))? {
            artifacts.push(artifact);
        }
    }
    Ok(artifacts)
}

/// Validates one thin Mach-O and returns it only for a supported identity.
fn parse_supported_slice(payload: bytes::Bytes) -> Result<Option<Artifact>, RuntimeError> {
    let bytes = payload.as_ref();
    if !artifact_size_allowed(bytes.len()) {
        return Err(invalid_artifact());
    }
    if FileKind::parse(bytes).map_err(|_| invalid_artifact())? != FileKind::MachO64 {
        return Ok(None);
    }
    let file = MachOFile64::<object::Endianness>::parse(bytes).map_err(|_| invalid_artifact())?;
    let architecture = match (file.architecture(), file.sub_architecture()) {
        (Architecture::Aarch64, Some(SubArchitecture::Arm64E)) => NativeArchitecture::Arm64E,
        (Architecture::Aarch64, _) => NativeArchitecture::Arm64,
        (Architecture::X86_64, _) => NativeArchitecture::X86_64,
        _ => return Ok(None),
    };
    let has_usable_debug_info = file.sections().any(|section| {
        section.name().ok() == Some("__debug_info")
            && section.kind() == SectionKind::Debug
            && section.data().is_ok_and(|data| !data.is_empty())
    });
    if !file.has_debug_symbols() || !has_usable_debug_info {
        return Err(invalid_artifact());
    }
    let uuid = file
        .mach_uuid()
        .map_err(|_| invalid_artifact())?
        .filter(|uuid| uuid.iter().any(|byte| *byte != 0))
        .ok_or_else(invalid_artifact)?;
    Ok(Some(Artifact {
        image_uuid: format_uuid(uuid),
        architecture,
        kind: NativeArtifactType::AppleDsym,
        sha256: sha256_hex(bytes),
        bytes: payload,
    }))
}

/// Returns whether one exact thin object fits the public per-part contract.
const fn artifact_size_allowed(size: usize) -> bool {
    size > 0 && size <= MAX_ARTIFACT_BYTES
}

/// Formats one Mach-O UUID in canonical lowercase dashed form.
fn format_uuid(bytes: [u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

/// Computes lowercase SHA-256 for exactly the bytes sent in one part.
fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

/// Returns the fixed path-free local artifact error.
const fn invalid_artifact() -> RuntimeError {
    RuntimeError::NativeDebugArtifactInvalid
}

#[cfg(test)]
mod tests {
    use super::{
        Artifact, MAX_ARTIFACT_BYTES, MAX_SOURCE_BYTES, NativeArchitecture, NativeArtifactType,
        RESUMABLE_CHUNK_BYTES, artifact_size_allowed, finalize, sha256_hex,
        validate_expected_uuids, zip_expansion_is_unsafe,
    };
    /// Proves fixed chunk boundaries and cheap immutable slices without a large fixture.
    #[test]
    fn resumable_chunks_are_ordered_and_share_backing_storage() {
        let bytes = bytes::Bytes::from(vec![0x5a; RESUMABLE_CHUNK_BYTES + 7]);
        let artifact = Artifact {
            image_uuid: String::from("10111213-1415-1617-1819-1a1b1c1d1e1f"),
            architecture: NativeArchitecture::Arm64,
            kind: NativeArtifactType::AppleDsym,
            sha256: sha256_hex(bytes.as_ref()),
            bytes,
        };
        let chunks = artifact.resumable_chunks();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].bytes.len(), RESUMABLE_CHUNK_BYTES);
        assert_eq!(chunks[1].bytes.len(), 7);
        assert_eq!(chunks[0].bytes.as_ptr(), artifact.bytes.as_ptr());
        assert_eq!(
            chunks[1].bytes.as_ptr(),
            artifact.bytes[RESUMABLE_CHUNK_BYTES..].as_ptr()
        );
    }

    /// Proves per-object size checks without allocating the maximum payload.
    #[test]
    fn artifact_size_boundary_is_exact() {
        assert!(!artifact_size_allowed(0));
        assert!(artifact_size_allowed(MAX_ARTIFACT_BYTES));
        assert!(!artifact_size_allowed(MAX_ARTIFACT_BYTES + 1));
    }

    /// Proves realistic universal iOS dSYMs fit without weakening finite bounds.
    #[test]
    fn large_ios_debug_objects_fit_the_public_upload_boundary() {
        const LARGE_THIN_OBJECT_BYTES: usize = 224 * 1024 * 1024;

        const {
            assert!(artifact_size_allowed(LARGE_THIN_OBJECT_BYTES));
            assert!(MAX_SOURCE_BYTES >= LARGE_THIN_OBJECT_BYTES * 2);
        }
    }

    /// Proves compressed-size policy from synthetic lengths without a ZIP bomb fixture.
    #[test]
    fn zip_expansion_boundary_is_bounded() {
        assert!(!zip_expansion_is_unsafe(1024, 1024 * 100));
        assert!(zip_expansion_is_unsafe(1, 2 * 1024 * 1024));
    }

    /// Proves optional release UUID gating is exact and order-independent.
    #[test]
    fn expected_uuid_gate_is_optional_and_exact() {
        let artifacts = vec![fixture_artifact(1), fixture_artifact(2)];
        assert!(validate_expected_uuids(artifacts.as_slice(), &[]).is_ok());
        assert!(
            validate_expected_uuids(
                artifacts.as_slice(),
                &[
                    String::from("00000000-0000-0000-0000-000000000002"),
                    String::from("00000000-0000-0000-0000-000000000001"),
                ],
            )
            .is_ok()
        );
        assert!(
            validate_expected_uuids(
                artifacts.as_slice(),
                &[String::from("00000000-0000-0000-0000-000000000001")],
            )
            .is_err()
        );
    }

    /// Proves artifact count enforcement without large files.
    #[test]
    fn artifact_count_overflow_is_rejected() {
        let artifacts = (0..51).map(fixture_artifact).collect::<Vec<_>>();
        assert!(finalize(artifacts).is_err());
    }

    /// Builds one tiny valid-internal identity for pure policy tests.
    fn fixture_artifact(index: usize) -> Artifact {
        Artifact {
            image_uuid: format!("00000000-0000-0000-0000-{index:012x}"),
            architecture: NativeArchitecture::Arm64,
            kind: NativeArtifactType::AppleDsym,
            sha256: String::from(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            bytes: bytes::Bytes::from_static(b"debug"),
        }
    }
}
