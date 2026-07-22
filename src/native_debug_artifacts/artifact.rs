//! Local Apple debug-object discovery and validation.

use crate::RuntimeError;
use object::read::macho::{MachOFatFile, MachOFatFile32, MachOFatFile64, MachOFile64};
use object::{
    Architecture, FileKind, Object as _, ObjectSection as _, SectionKind, SubArchitecture,
};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Maximum number of exact object identities accepted by one upload.
const MAX_ARTIFACTS: usize = 50;
/// Maximum bytes accepted for one uploaded thin debug object.
pub(super) const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;
/// Maximum bytes accepted for one source file before slice extraction.
const MAX_SOURCE_BYTES: usize = 128 * 1024 * 1024;

/// One supported architecture accepted by the public contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NativeArchitecture {
    /// Standard Apple arm64.
    Arm64,
    /// Pointer-authenticated Apple arm64e.
    Arm64E,
    /// Intel x86-64.
    X86_64,
}

impl NativeArchitecture {
    /// Returns the canonical public API value.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Arm64 => "arm64",
            Self::Arm64E => "arm64e",
            Self::X86_64 => "x86_64",
        }
    }
}

/// One validated thin Mach-O payload and its exact public identity.
pub(super) struct Artifact {
    /// Canonical lowercase image UUID.
    pub(super) image_uuid: String,
    /// Canonical supported architecture.
    pub(super) architecture: NativeArchitecture,
    /// Lowercase SHA-256 of exactly the uploaded bytes.
    pub(super) sha256: String,
    /// Exact thin Mach-O bytes uploaded for this identity.
    pub(super) bytes: bytes::Bytes,
}

impl Artifact {
    /// Returns the exact uploaded byte count.
    pub(super) fn byte_size(&self) -> u64 {
        u64::try_from(self.bytes.len()).unwrap_or(u64::MAX)
    }

    /// Returns a cheap immutable handle for multipart construction or auth replay.
    pub(super) fn multipart_payload(&self) -> bytes::Bytes {
        self.bytes.clone()
    }
}

/// Enumerates one file or dSYM bundle without retaining local names.
pub(super) fn collect(path: &Path) -> Result<Vec<Artifact>, RuntimeError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid_artifact())?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_artifact());
    }
    let files = if metadata.is_file() {
        vec![path.to_path_buf()]
    } else if metadata.is_dir()
        && path.extension().and_then(std::ffi::OsStr::to_str) == Some("dSYM")
    {
        let dwarf = path.join("Contents/Resources/DWARF");
        collect_regular_files(dwarf.as_path())?
    } else {
        return Err(invalid_artifact());
    };
    if files.is_empty() {
        return Err(invalid_artifact());
    }

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
    if artifacts.is_empty() || artifacts.len() > MAX_ARTIFACTS {
        return Err(invalid_artifact());
    }
    Ok(artifacts)
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
    let before = std::fs::symlink_metadata(path).map_err(|_| invalid_artifact())?;
    if before.file_type().is_symlink()
        || !before.is_file()
        || usize::try_from(before.len()).map_or(true, |length| length > MAX_SOURCE_BYTES)
    {
        return Err(invalid_artifact());
    }
    let mut file = std::fs::File::open(path).map_err(|_| invalid_artifact())?;
    let opened = file.metadata().map_err(|_| invalid_artifact())?;
    if !opened.is_file() || !same_file(&before, &opened) {
        return Err(invalid_artifact());
    }
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
        || !same_file(&opened, &after)
    {
        return Err(invalid_artifact());
    }
    Ok(bytes)
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
    if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES {
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
        sha256: sha256_hex(bytes),
        bytes: payload,
    }))
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
    use super::{Artifact, NativeArchitecture};

    /// Proves multipart replay shares immutable payload storage instead of deep-copying it.
    #[test]
    fn multipart_payload_clone_reuses_backing_storage() {
        let artifact = Artifact {
            image_uuid: String::from("10111213-1415-1617-1819-1a1b1c1d1e1f"),
            architecture: NativeArchitecture::Arm64,
            sha256: String::from(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            bytes: bytes::Bytes::from_static(b"debug"),
        };
        let replay = artifact.multipart_payload();
        assert_eq!(artifact.bytes.as_ptr(), replay.as_ptr());
        assert_eq!(artifact.bytes.len(), replay.len());
    }
}
