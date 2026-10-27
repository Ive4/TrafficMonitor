//! Owned snapshots of Windows network interfaces and 64-bit byte counters.

use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkAdapter {
    /// The interface LUID, independent of enumeration order or interface index.
    pub id: u64,
    pub interface_index: u32,
    pub alias: String,
    pub description: String,
    pub received_bytes: u64,
    pub sent_bytes: u64,
    pub operational: bool,
    pub is_loopback: bool,
}

/// Returns all logical and physical interfaces, including loopback and down ones.
///
/// Strings and counters are copied before the native table is freed. Selection
/// and aggregation policy belong to the caller. LUIDs identify interfaces on the
/// current system; they should not be treated as portable configuration IDs.
#[cfg(windows)]
pub fn snapshot() -> io::Result<Vec<NetworkAdapter>> {
    windows::snapshot()
}

/// Network sampling requires Windows; no synthetic sample is returned.
#[cfg(not(windows))]
pub fn snapshot() -> io::Result<Vec<NetworkAdapter>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Network sampling requires Windows",
    ))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ptr::NonNull;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_ROW2, MIB_IF_TABLE2,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;

    /// Owns exactly one allocation returned by a successful GetIfTable2 call.
    ///
    /// Win32 documents that FreeMibTable is the required matching deallocator:
    /// https://learn.microsoft.com/en-us/windows/win32/api/netioapi/nf-netioapi-getiftable2
    struct InterfaceTable(NonNull<MIB_IF_TABLE2>);

    impl InterfaceTable {
        fn acquire() -> io::Result<Self> {
            let mut table = std::ptr::null_mut();
            // SAFETY: GetIfTable2 receives a valid writable out-pointer. On
            // success the returned allocation transfers to the RAII owner below.
            let status = unsafe { GetIfTable2(&mut table) };
            if status != 0 {
                // GetIfTable2 returns its error directly, not via GetLastError.
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            NonNull::new(table).map(Self).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "GetIfTable2 returned a null table",
                )
            })
        }

        fn rows(&self) -> io::Result<&[MIB_IF_ROW2]> {
            let table = self.0.as_ptr();
            // SAFETY: The successful call provided the initialized table header,
            // owned by self. Reading the field avoids forming a reference to the
            // declared one-element trailing array when the row count is zero.
            let count = unsafe { std::ptr::addr_of!((*table).NumEntries).read() } as usize;
            if count == 0 {
                return Ok(&[]);
            }
            let byte_size = count.checked_mul(std::mem::size_of::<MIB_IF_ROW2>());
            if byte_size.is_none_or(|size| size > isize::MAX as usize) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Interface table is too large",
                ));
            }
            // SAFETY: GetIfTable2 allocates and initializes NumEntries rows. The
            // repr(C) field address and MIB_IF_ROW2 stride preserve SDK padding.
            // The byte-size check satisfies slice size requirements, and the
            // returned borrow cannot outlive self or its FreeMibTable call.
            unsafe {
                let rows = std::ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>();
                Ok(std::slice::from_raw_parts(rows, count))
            }
        }
    }

    impl Drop for InterfaceTable {
        fn drop(&mut self) {
            // SAFETY: This pointer came from GetIfTable2 and has exactly one
            // owner. No rows can remain borrowed after this owner is dropped.
            unsafe { FreeMibTable(self.0.as_ptr().cast()) };
        }
    }

    pub(super) fn snapshot() -> io::Result<Vec<NetworkAdapter>> {
        let table = InterfaceTable::acquire()?;
        Ok(table.rows()?.iter().map(from_row).collect())
    }

    fn from_row(row: &MIB_IF_ROW2) -> NetworkAdapter {
        let text = |value: &[u16]| {
            let end = value
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(value.len());
            String::from_utf16_lossy(&value[..end])
        };
        NetworkAdapter {
            // SAFETY: NET_LUID_LH is the SDK's union of a u64 Value and its
            // bitfields; every initialized row has a valid u64 representation.
            id: unsafe { row.InterfaceLuid.Value },
            interface_index: row.InterfaceIndex,
            alias: text(&row.Alias),
            description: text(&row.Description),
            received_bytes: row.InOctets,
            sent_bytes: row.OutOctets,
            operational: row.OperStatus == IfOperStatusUp,
            is_loopback: row.Type == IF_TYPE_SOFTWARE_LOOPBACK,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::NetworkManagement::IpHelper::IF_TYPE_SOFTWARE_LOOPBACK;
        use windows_sys::Win32::NetworkManagement::Ndis::{
            IfOperStatusDown, IfOperStatusUp, NET_LUID_LH,
        };

        #[test]
        fn preserves_luid_independently_of_index_and_large_byte_counters() {
            let row = MIB_IF_ROW2 {
                InterfaceLuid: NET_LUID_LH {
                    Value: 0x1234_5678_9abc_def0,
                },
                InterfaceIndex: 37,
                InOctets: u64::from(u32::MAX) + 123,
                OutOctets: u64::MAX - 3,
                ..Default::default()
            };
            let adapter = from_row(&row);
            assert_eq!(adapter.id, 0x1234_5678_9abc_def0);
            assert_eq!(adapter.interface_index, 37);
            assert_eq!(adapter.received_bytes, u64::from(u32::MAX) + 123);
            assert_eq!(adapter.sent_bytes, u64::MAX - 3);
        }

        #[test]
        fn decodes_utf16_names_and_stops_at_nul() {
            let mut row = MIB_IF_ROW2::default();
            let alias: Vec<_> = "以太网 🚀".encode_utf16().collect();
            row.Alias[..alias.len()].copy_from_slice(&alias);
            row.Alias[alias.len() + 1] = b'X' as u16;
            let description: Vec<_> = "Adapter 描述".encode_utf16().collect();
            row.Description[..description.len()].copy_from_slice(&description);
            let adapter = from_row(&row);
            assert_eq!(adapter.alias, "以太网 🚀");
            assert_eq!(adapter.description, "Adapter 描述");
        }

        #[test]
        fn malformed_or_unterminated_name_is_bounded_and_lossy() {
            let row = MIB_IF_ROW2 {
                Alias: [0xd800; 257],
                Description: [b'A' as u16; 257],
                ..Default::default()
            };
            let adapter = from_row(&row);
            assert_eq!(adapter.alias, "\u{fffd}".repeat(257));
            assert_eq!(adapter.description, "A".repeat(257));
        }

        #[test]
        fn only_operational_up_is_marked_operational() {
            let row = MIB_IF_ROW2 {
                OperStatus: IfOperStatusUp,
                ..Default::default()
            };
            assert!(from_row(&row).operational);
            let row = MIB_IF_ROW2 {
                OperStatus: IfOperStatusDown,
                ..row
            };
            assert!(!from_row(&row).operational);
        }

        #[test]
        fn loopback_is_identified_without_filtering_other_interfaces() {
            let row = MIB_IF_ROW2 {
                Type: IF_TYPE_SOFTWARE_LOOPBACK,
                ..Default::default()
            };
            assert!(from_row(&row).is_loopback);
            let row = MIB_IF_ROW2 { Type: 6, ..row };
            assert!(!from_row(&row).is_loopback);
        }

        #[test]
        fn snapshots_own_data_after_native_table_is_released() {
            let first = snapshot().expect("GetIfTable2 should succeed");
            let copy = first.clone();
            for _ in 0..8 {
                let _next = snapshot().expect("repeated table allocation should succeed");
            }
            assert_eq!(first, copy);
            let mut ids = std::collections::HashSet::new();
            for adapter in first {
                assert!(ids.insert(adapter.id), "interface LUIDs should be unique");
                assert_ne!(adapter.id, 0);
            }
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_system_does_not_fabricate_adapters() {
        assert_eq!(snapshot().unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
}
