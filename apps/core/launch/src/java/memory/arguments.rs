// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::{BYTES_PER_MIB, HeapDefaults};

pub(crate) fn jvm_defaults(args: &[String], heap: HeapDefaults) -> [Option<String>; 2] {
    let initial = heap_argument(args, ["-Xms", "-XX:InitialHeapSize="]);
    let maximum = heap_argument(args, ["-Xmx", "-XX:MaxHeapSize="]);
    let maximum_from_ram = maximum.is_none()
        && args.iter().any(|arg| {
            [
                "-XX:MaxRAMPercentage=",
                "-XX:MinRAMPercentage=",
                "-XX:MaxRAMFraction=",
                "-XX:MinRAMFraction=",
                "-XX:MaxRAM=",
            ]
            .iter()
            .any(|prefix| arg.starts_with(prefix))
        });
    if maximum_from_ram {
        // The JVM sets both bounds when the manifest delegates an unknown maximum to it.
        return [None, None];
    }
    let initial_from_ram = args.iter().any(|arg| {
        arg.starts_with("-XX:InitialRAMPercentage=") || arg.starts_with("-XX:InitialRAMFraction=")
    });
    let minimum = heap_argument(args, ["-XX:MinHeapSize="]);
    let soft_maximum = heap_argument(args, ["-XX:SoftMaxHeapSize="]);
    let default_initial = heap
        .initial_mib
        .filter(|_| initial.is_none() && !initial_from_ram)
        .map(|mib| initial_argument(mib, minimum, maximum));
    let default_maximum = maximum.is_none().then(|| {
        let mib = heap.maximum_mib;
        let minimum = [initial, minimum, soft_maximum]
            .into_iter()
            .flatten()
            .filter_map(|size| Some((size_in_bytes(size)?, size)))
            .max_by_key(|(bytes, _)| *bytes);
        match minimum.filter(|(bytes, _)| *bytes > mib_bytes(mib)) {
            Some((_, size)) => format!("-Xmx{size}"),
            None => format!("-Xmx{mib}m"),
        }
    });
    [default_initial, default_maximum]
}

fn initial_argument(mib: u32, minimum: Option<&str>, maximum: Option<&str>) -> String {
    let default = format!("{mib}m");
    let constrained = minimum
        .filter(|size| size_in_bytes(size).is_some_and(|bytes| bytes > mib_bytes(mib)))
        .unwrap_or(&default);
    let initial = maximum
        .filter(|max| {
            size_in_bytes(max)
                .zip(size_in_bytes(constrained))
                .is_some_and(|(max, initial)| max < initial)
        })
        .unwrap_or(constrained);
    format!("-Xms{initial}")
}

fn heap_argument<'a, const N: usize>(args: &'a [String], prefixes: [&str; N]) -> Option<&'a str> {
    args.iter()
        .rev()
        .find_map(|arg| prefixes.iter().find_map(|prefix| arg.strip_prefix(prefix)))
}

fn mib_bytes(mib: u32) -> u64 {
    u64::from(mib) * BYTES_PER_MIB
}

fn size_in_bytes(size: &str) -> Option<u64> {
    let (digits, unit) = size.split_at_checked(size.len().checked_sub(1)?)?;
    let shift = match unit.as_bytes() {
        [b'k' | b'K'] => 10,
        [b'm' | b'M'] => 20,
        [b'g' | b'G'] => 30,
        [b't' | b'T'] => 40,
        _ => return size.parse().ok(),
    };
    digits.parse::<u64>().ok()?.checked_mul(1 << shift)
}
