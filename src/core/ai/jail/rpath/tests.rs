use std::path::{Path, PathBuf};

use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn paths(items: &[&str]) -> Vec<PathBuf> {
    items.iter().map(|s| p(s)).collect()
}

const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;
const LOAD_SHIFT: u64 = 0x1000;

/// A little-endian ELF64 image with a dynamic section. The string table is at
/// file offset 0x400 and its virtual address is shifted by 0x1000, so a reader
/// that mixes the two up finds nothing. Each of `strings` is NUL-terminated
/// after `libc.so.6`; the dynamic entry for a string is `(tag, index)`.
struct Image {
    bytes: Vec<u8>,
}

fn put(img: &mut [u8], at: usize, data: &[u8]) {
    img[at..at + data.len()].copy_from_slice(data);
}

fn image(entries: &[(u64, &str)], load_shift: u64) -> Image {
    let mut img = vec![0u8; 0x1000];
    put(&mut img, 0, b"\x7fELF\x02\x01\x01");
    put(&mut img, 0x20, &64u64.to_le_bytes());
    put(&mut img, 0x36, &56u16.to_le_bytes());
    put(&mut img, 0x38, &2u16.to_le_bytes());
    // String table: an empty name, a needed library, then one string each.
    let mut strtab = b"\0libc.so.6\0".to_vec();
    let mut dynamic: Vec<(u64, u64)> = vec![(DT_NEEDED, 1)];
    let strtab_vaddr = 0x400 + load_shift;
    dynamic.push((DT_STRTAB, strtab_vaddr));
    for (tag, text) in entries {
        dynamic.push((*tag, strtab.len() as u64));
        strtab.extend_from_slice(text.as_bytes());
        strtab.push(0);
    }
    dynamic.push((0, 0));
    put(&mut img, 0x400, &strtab);
    let mut dyn_bytes = Vec::new();
    for (tag, val) in &dynamic {
        dyn_bytes.extend_from_slice(&tag.to_le_bytes());
        dyn_bytes.extend_from_slice(&val.to_le_bytes());
    }
    put(&mut img, 0x200, &dyn_bytes);
    // PT_LOAD over the whole file, then PT_DYNAMIC.
    let phdr = |img: &mut [u8], n: usize, kind: u32, offset: u64, vaddr: u64, size: u64| {
        let at = 64 + n * 56;
        put(img, at, &kind.to_le_bytes());
        put(img, at + 8, &offset.to_le_bytes());
        put(img, at + 16, &vaddr.to_le_bytes());
        put(img, at + 32, &size.to_le_bytes());
    };
    phdr(&mut img, 0, 1, 0, load_shift, 0x1000);
    phdr(
        &mut img,
        1,
        2,
        0x200,
        0x200 + load_shift,
        dyn_bytes.len() as u64,
    );
    Image { bytes: img }
}

/// An ELF64 image whose own search path is `rpath`, for the tests of the
/// runtime that run a program with a library path of its own.
pub(crate) fn image_with_rpath(rpath: &str) -> Vec<u8> {
    image(&[(DT_RPATH, rpath)], LOAD_SHIFT).bytes
}

fn dirs_of(img: &Image) -> Vec<PathBuf> {
    dirs_in(img, "/opt/x/bin")
}

fn dirs_in(img: &Image, origin: &str) -> Vec<PathBuf> {
    let head = &img.bytes[..4096];
    search_dirs(head, Path::new(origin), &|offset, len| {
        let start = usize::try_from(offset).ok()?;
        let rest = img.bytes.get(start..)?;
        Some(rest[..len.min(rest.len())].to_vec())
    })
}

#[test]
fn an_rpath_is_split_and_origin_is_the_folder_of_the_executable() {
    let img = image(
        &[(DT_RPATH, "$ORIGIN/../lib:$ORIGIN/:/opt/dep/lib:${ORIGIN}/x")],
        LOAD_SHIFT,
    );
    assert_eq!(
        dirs_of(&img),
        paths(&["/opt/x/lib", "/opt/x/bin", "/opt/dep/lib", "/opt/x/bin/x"])
    );
}

#[test]
fn a_runpath_wins_over_an_rpath_as_the_loader_does() {
    let img = image(
        &[(DT_RPATH, "/old/lib"), (DT_RUNPATH, "/new/lib")],
        LOAD_SHIFT,
    );
    assert_eq!(dirs_of(&img), paths(&["/new/lib"]));
}

#[test]
fn only_an_rpath_is_used_when_there_is_no_runpath() {
    let img = image(&[(DT_RPATH, "/old/lib")], LOAD_SHIFT);
    assert_eq!(dirs_of(&img), paths(&["/old/lib"]));
}

#[test]
fn entries_that_are_not_plain_absolute_folders_are_dropped_and_repeats_removed() {
    let img = image(
        &[(
            DT_RPATH,
            "/a/lib::relative/lib:.:/b/$LIB:/c/${PLATFORM}/x:/a/lib:/d/lib",
        )],
        LOAD_SHIFT,
    );
    // The empty entry means the current folder, relative ones depend on it,
    // and $LIB or $PLATFORM depend on the machine: none is granted.
    assert_eq!(dirs_of(&img), paths(&["/a/lib", "/d/lib"]));
}

#[test]
fn no_search_path_means_no_folders() {
    assert_eq!(dirs_of(&image(&[], LOAD_SHIFT)), Vec::<PathBuf>::new());
}

#[test]
fn something_that_is_not_a_little_endian_elf64_gives_nothing() {
    let read = |_: u64, _: usize| None;
    let go = |head: &[u8]| search_dirs(head, Path::new("/o"), &read);
    assert!(go(b"#!/usr/bin/env node\n").is_empty());
    assert!(go(b"").is_empty());
    let mut be = image(&[(DT_RPATH, "/a")], LOAD_SHIFT).bytes;
    be[5] = 2;
    assert!(go(&be).is_empty());
    let mut c32 = image(&[(DT_RPATH, "/a")], LOAD_SHIFT).bytes;
    c32[4] = 1;
    assert!(go(&c32).is_empty());
}

#[test]
fn a_string_table_outside_every_load_segment_gives_nothing() {
    // The table's address is not mapped by any PT_LOAD, so it cannot be
    // turned into a file offset.
    let mut img = image(&[(DT_RPATH, "/a")], LOAD_SHIFT);
    let load_size_at = 64 + 32;
    img.bytes[load_size_at..load_size_at + 8].copy_from_slice(&0x100u64.to_le_bytes());
    assert!(dirs_of(&img).is_empty());
}

#[test]
fn a_dynamic_section_that_cannot_be_read_gives_nothing() {
    let img = image(&[(DT_RPATH, "/a")], LOAD_SHIFT);
    let head = &img.bytes[..4096];
    let none = |_: u64, _: usize| None;
    assert!(search_dirs(head, Path::new("/o"), &none).is_empty());
}

#[test]
fn a_string_without_its_nul_is_not_trusted() {
    let mut img = image(&[(DT_RPATH, "/a/lib")], LOAD_SHIFT);
    // Overwrite the NUL that ends the string and everything after it.
    let end = 0x400 + b"\0libc.so.6\0/a/lib".len();
    for b in &mut img.bytes[end..] {
        *b = b'x';
    }
    assert!(dirs_of(&img).is_empty());
}

#[test]
fn a_string_longer_than_the_cap_gives_nothing() {
    let img = image(&[(DT_RPATH, "/a/lib")], LOAD_SHIFT);
    let head = &img.bytes[..4096];
    // The string table answers with path text that never ends.
    let endless = |offset: u64, len: usize| {
        let start = usize::try_from(offset).ok()?;
        if start >= 0x400 {
            return Some(vec![b'a'; len]);
        }
        let rest = img.bytes.get(start..)?;
        Some(rest[..len.min(rest.len())].to_vec())
    };
    assert!(search_dirs(head, Path::new("/o"), &endless).is_empty());
}

#[test]
fn the_dynamic_section_is_read_from_the_file_not_from_the_head() {
    // Same image, but the reader only answers for the offsets it was asked.
    let img = image(&[(DT_RPATH, "/a/lib")], LOAD_SHIFT);
    let asked = std::cell::RefCell::new(Vec::new());
    let head = &img.bytes[..4096];
    let dirs = search_dirs(head, Path::new("/o"), &|offset, len| {
        asked.borrow_mut().push(offset);
        let start = usize::try_from(offset).ok()?;
        let rest = img.bytes.get(start..)?;
        Some(rest[..len.min(rest.len())].to_vec())
    });
    assert_eq!(dirs, paths(&["/a/lib"]));
    let asked = asked.into_inner();
    assert!(asked.contains(&0x200), "the dynamic section: {asked:?}");
    assert!(
        asked.iter().any(|o| (0x400..0x500).contains(o)),
        "the string, by file offset and not by address: {asked:?}"
    );
}

#[test]
fn homebrew_keeps_its_openssl_config_and_cert_bundle_next_to_the_prefix() {
    let dirs = paths(&[
        "/home/linuxbrew/.linuxbrew/Cellar/node/26.8.1/lib",
        "/home/linuxbrew/.linuxbrew/opt/zlib/lib",
        "/home/linuxbrew/.linuxbrew/opt/openssl@3/lib",
        "/home/linuxbrew/.linuxbrew/lib",
    ]);
    assert_eq!(
        homebrew_openssl_files(&dirs),
        paths(&[
            "/home/linuxbrew/.linuxbrew/etc/openssl@3/openssl.cnf",
            "/home/linuxbrew/.linuxbrew/etc/openssl@3/cert.pem",
        ])
    );
}

#[test]
fn only_the_opt_openssl_lib_shape_names_those_files() {
    for dir in [
        "/usr/lib/openssl@3/lib",
        "/x/opt/openssl@3",
        "/x/opt/openssl@3/lib/sub",
        "/x/opt/openssl/lib",
        "/x/opt/openssl@/lib",
        "/x/opt/openssl@3x/lib",
        "/x/opt/openssl@../lib",
        "/x/opt/other@3/lib",
    ] {
        assert!(homebrew_openssl_files(&paths(&[dir])).is_empty(), "{dir}");
    }
    assert_eq!(
        homebrew_openssl_files(&paths(&["/x/opt/openssl@3/lib", "/x/opt/openssl@3/lib"])).len(),
        2,
        "a repeated folder gives its files once"
    );
}

#[test]
fn a_load_segment_that_would_overflow_maps_nothing() {
    let loads = [Load {
        vaddr: 0x1000,
        offset: u64::MAX - 2,
        size: 0x10,
    }];
    assert_eq!(file_offset(&loads, 0x1001), Some(u64::MAX - 1));
    assert_eq!(file_offset(&loads, 0x1008), None);
}

#[test]
fn an_env_shebang_needs_env_on_macos() {
    for head in [
        &b"#!/usr/bin/env node\nconsole.log(1)\n"[..],
        b"#!/usr/bin/env -S node --no-warnings\n",
        b"#!/usr/bin/env node",
    ] {
        assert_eq!(
            shebang_env(Os::Macos, head),
            Some(p("/usr/bin/env")),
            "{}",
            String::from_utf8_lossy(head)
        );
    }
}

#[test]
fn only_an_env_shebang_on_macos_needs_env() {
    let head = b"#!/usr/bin/env node\n";
    assert_eq!(shebang_env(Os::Linux, head), None);
    assert_eq!(shebang_env(Os::Other, head), None);
    for head in [
        &b"#!/bin/sh\necho\n"[..],
        b"#!/opt/node/bin/node\n",
        b"\x7fELF\x02\x01\x01",
        b"#!env node\n",
        b"",
    ] {
        assert_eq!(shebang_env(Os::Macos, head), None);
    }
}

#[test]
fn a_homebrew_node_gets_its_openssl_config_on_macos() {
    assert_eq!(
        macos_openssl_files(Os::Macos, &p("/opt/homebrew/Cellar/node/26.8.1/bin/node")),
        paths(&["/opt/homebrew/etc/openssl@3/openssl.cnf"])
    );
    assert_eq!(
        macos_openssl_files(Os::Macos, &p("/usr/local/Cellar/node@22/22.1.0/bin/node")),
        paths(&["/usr/local/etc/openssl@3/openssl.cnf"])
    );
}

#[test]
fn no_other_node_gets_an_openssl_config() {
    let homebrew = p("/opt/homebrew/Cellar/node/26.8.1/bin/node");
    assert!(macos_openssl_files(Os::Linux, &homebrew).is_empty());
    assert!(macos_openssl_files(Os::Other, &homebrew).is_empty());
    for node in [
        "/Users/u/.nvm/versions/node/v24/bin/node",
        "/Users/u/opt/homebrew/Cellar/node/26/bin/node",
        "/opt/homebrew/bin/node",
        "/opt/homebrew/Cellar/node",
        "/usr/local/bin/node",
    ] {
        assert!(
            macos_openssl_files(Os::Macos, &p(node)).is_empty(),
            "{node}"
        );
    }
}
