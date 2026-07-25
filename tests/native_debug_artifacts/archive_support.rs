//! Upload-only dSYM archive integration fixtures.

use std::ffi::{OsStr, OsString};

use crate::support::upload_args;

pub(crate) fn dsym_upload_args(path: &OsStr, image_uuids: &[&str], dry_run: bool) -> Vec<OsString> {
    let mut args = upload_args(path);
    let _json = args.pop();
    for image_uuid in image_uuids {
        args.push(OsString::from("--expect-image-uuid"));
        args.push(OsString::from(image_uuid));
    }
    if dry_run {
        args.push(OsString::from("--dry-run"));
    }
    args.push(OsString::from("--json"));
    args
}

pub(crate) struct ZipFixtureEntry<'a> {
    name: &'a str,
    bytes: &'a [u8],
    unix_mode: u32,
}

impl<'a> ZipFixtureEntry<'a> {
    pub(crate) fn file(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            bytes,
            unix_mode: 0o100_644,
        }
    }

    pub(crate) fn symlink(name: &'a str, target: &'a [u8]) -> Self {
        Self {
            name,
            bytes: target,
            unix_mode: 0o120_777,
        }
    }
}

pub(crate) fn stored_zip(
    entries: &[ZipFixtureEntry<'_>],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut archive = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = u32::try_from(archive.len())?;
        let name = entry.name.as_bytes();
        let name_len = u16::try_from(name.len())?;
        let size = u32::try_from(entry.bytes.len())?;
        let checksum = crc32(entry.bytes);

        archive.extend_from_slice(0x0403_4b50u32.to_le_bytes().as_slice());
        for value in [20u16, 0, 0, 0, 0] {
            archive.extend_from_slice(value.to_le_bytes().as_slice());
        }
        for value in [checksum, size, size] {
            archive.extend_from_slice(value.to_le_bytes().as_slice());
        }
        archive.extend_from_slice(name_len.to_le_bytes().as_slice());
        archive.extend_from_slice(0u16.to_le_bytes().as_slice());
        archive.extend_from_slice(name);
        archive.extend_from_slice(entry.bytes);

        central.extend_from_slice(0x0201_4b50u32.to_le_bytes().as_slice());
        for value in [(3u16 << 8) | 20, 20, 0, 0, 0, 0] {
            central.extend_from_slice(value.to_le_bytes().as_slice());
        }
        for value in [checksum, size, size] {
            central.extend_from_slice(value.to_le_bytes().as_slice());
        }
        for value in [name_len, 0, 0, 0, 0] {
            central.extend_from_slice(value.to_le_bytes().as_slice());
        }
        central.extend_from_slice((entry.unix_mode << 16).to_le_bytes().as_slice());
        central.extend_from_slice(offset.to_le_bytes().as_slice());
        central.extend_from_slice(name);
    }
    let central_offset = u32::try_from(archive.len())?;
    let central_size = u32::try_from(central.len())?;
    archive.extend_from_slice(central.as_slice());
    archive.extend_from_slice(0x0605_4b50u32.to_le_bytes().as_slice());
    for value in [
        0u16,
        0,
        u16::try_from(entries.len())?,
        u16::try_from(entries.len())?,
    ] {
        archive.extend_from_slice(value.to_le_bytes().as_slice());
    }
    archive.extend_from_slice(central_size.to_le_bytes().as_slice());
    archive.extend_from_slice(central_offset.to_le_bytes().as_slice());
    archive.extend_from_slice(0u16.to_le_bytes().as_slice());
    Ok(archive)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}
