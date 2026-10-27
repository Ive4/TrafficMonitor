//! Extended hardware counters and IP network details.
//! Thread-affine, read-only native hardware counters and owned IP adapter details.
use std::{collections::BTreeMap, io, marker::PhantomData, rc::Rc};

#[derive(Debug, Default, Clone)]
pub struct HardwareSnapshot {
    pub cpu_usage_percent: Option<f64>,
    pub cpu_frequency_mhz: Option<f64>,
    pub cpu_frequencies_mhz: Vec<u32>,
    pub gpu_usage_percent: Option<f64>,
    pub disk_usage_percent: Option<f64>,
    pub disks: Vec<(String, f64)>,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Default, Clone)]
pub struct AdapterDetail {
    pub id: u64,
    pub interface_index: u32,
    pub alias: String,
    pub description: String,
    pub adapter_name: String,
    pub mac: String,
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub gateways: Vec<String>,
    pub mtu: u32,
    pub link_speed_bps: u64,
    pub physical: bool,
    pub operational: bool,
}
pub struct HardwareSampler {
    #[cfg(windows)]
    inner: windows::Sampler,
    _thread_affine: PhantomData<Rc<()>>,
}
impl Default for HardwareSampler {
    fn default() -> Self {
        Self::new()
    }
}
impl HardwareSampler {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            inner: windows::Sampler::new(),
            _thread_affine: PhantomData,
        }
    }
    pub fn sample(&mut self, disk_name: &str) -> HardwareSnapshot {
        #[cfg(windows)]
        {
            self.inner.sample(disk_name)
        }
        #[cfg(not(windows))]
        {
            let _ = disk_name;
            HardwareSnapshot {
                diagnostics: vec!["Advanced hardware sampling requires Windows".into()],
                ..HardwareSnapshot::default()
            }
        }
    }
}
pub fn network_details() -> io::Result<Vec<AdapterDetail>> {
    #[cfg(windows)]
    {
        windows::network_details()
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Adapter details require Windows",
        ))
    }
}

fn gpu_usage(values: &[(String, f64)]) -> Option<f64> {
    let mut groups: BTreeMap<&str, f64> = BTreeMap::new();
    for (name, value) in values {
        if !value.is_finite() || *value < 0.0 {
            continue;
        }
        if let Some((_, engine)) = name.rsplit_once("engtype_") {
            let engine = engine.split('#').next().unwrap_or(engine);
            if !engine.is_empty() {
                *groups.entry(engine).or_default() += value;
            }
        }
    }
    groups
        .values()
        .copied()
        .reduce(f64::max)
        .map(|v| v.clamp(0.0, 100.0))
}
fn disk_values(values: &[(String, f64)]) -> Vec<(String, f64)> {
    values
        .iter()
        .filter(|(_, idle)| idle.is_finite())
        .map(|(name, idle)| (name.clone(), 100.0 - idle.clamp(0.0, 100.0)))
        .collect()
}
fn selected_disk(disks: &[(String, f64)], name: &str) -> Option<f64> {
    disks
        .iter()
        .find(|(n, _)| !name.is_empty() && n == name)
        .or_else(|| disks.iter().find(|(n, _)| n == "_Total"))
        .map(|(_, value)| *value)
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        collections::HashSet,
        mem::size_of,
        net::{Ipv4Addr, Ipv6Addr},
        ptr,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA},
        NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
        Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKET_ADDRESS},
        System::{
            Performance::*,
            Power::{CallNtPowerInformation, PROCESSOR_POWER_INFORMATION, ProcessorInformation},
            Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount},
        },
    };
    const MAX_BUFFER: usize = 64 * 1024 * 1024;
    const MAX_NODES: usize = 65536;
    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }
    /// SDK structures contain u64/double members on both x86 and x64.
    struct Buffer {
        words: Vec<u64>,
    }
    impl Buffer {
        fn new(bytes: usize) -> io::Result<Self> {
            if bytes == 0 || bytes > MAX_BUFFER {
                return Err(invalid("Native buffer exceeds safety limit"));
            }
            Ok(Self {
                words: vec![0; bytes.div_ceil(8)],
            })
        }
        fn len(&self) -> usize {
            self.words.len() * 8
        }
        fn ptr<T>(&mut self) -> *mut T {
            self.words.as_mut_ptr().cast()
        }
        fn offset<T>(&self, p: *const T, bytes: usize) -> io::Result<usize> {
            let start = p as usize;
            let base = self.words.as_ptr() as usize;
            let offset = start
                .checked_sub(base)
                .ok_or_else(|| invalid("Native pointer precedes its buffer"))?;
            if offset.checked_add(bytes).is_none_or(|end| end > self.len()) {
                return Err(invalid("Native pointer is outside its buffer"));
            }
            Ok(offset)
        }
        fn read<T: Copy>(&self, p: *const T) -> io::Result<T> {
            let offset = self.offset(p, size_of::<T>())?;
            Ok(unsafe {
                self.words
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<T>()
                    // IP Helper packs some x86 linked nodes on 4-byte boundaries,
                    // although the Rust SDK union containing u64 has 8-byte alignment.
                    .read_unaligned()
            })
        }
        fn wide(&self, p: *const u16) -> io::Result<String> {
            if p.is_null() {
                return Ok(String::new());
            }
            let offset = self.offset(p, 2)?;
            if offset % 2 != 0 {
                return Err(invalid("Unaligned native UTF-16 string"));
            }
            let count = ((self.len() - offset) / 2).min(32768);
            let units = unsafe {
                std::slice::from_raw_parts(
                    self.words.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
                    count,
                )
            };
            let end = units
                .iter()
                .position(|&v| v == 0)
                .ok_or_else(|| invalid("Unterminated native UTF-16 string"))?;
            String::from_utf16(&units[..end]).map_err(|_| invalid("Invalid native UTF-16 string"))
        }
        fn ansi(&self, p: *const u8) -> io::Result<String> {
            if p.is_null() {
                return Ok(String::new());
            }
            let offset = self.offset(p, 1)?;
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    self.words.as_ptr().cast::<u8>().add(offset),
                    (self.len() - offset).min(32768),
                )
            };
            let end = bytes
                .iter()
                .position(|&v| v == 0)
                .ok_or_else(|| invalid("Unterminated native adapter name"))?;
            Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
        }
    }
    struct Counter {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
        baseline: Instant,
        scalar: bool,
    }
    fn formatted_value(value: &PDH_FMT_COUNTERVALUE) -> Option<f64> {
        if !matches!(value.CStatus, 0 | 1) {
            return None;
        }
        // All counters in this module request PDH_FMT_DOUBLE before reading this union.
        let number = unsafe { value.Anonymous.doubleValue };
        number.is_finite().then_some(number)
    }
    impl Counter {
        fn new(path: &str, scalar: bool) -> Result<Self, String> {
            let mut query = ptr::null_mut();
            let status = unsafe { PdhOpenQueryW(ptr::null(), 0, &mut query) };
            if status != 0 {
                return Err(format!("PdhOpenQueryW: 0x{status:08x}"));
            }
            let mut result = Self {
                query,
                counter: ptr::null_mut(),
                baseline: Instant::now(),
                scalar,
            };
            let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
            let status =
                unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut result.counter) };
            if status != 0 {
                return Err(format!("PdhAddEnglishCounterW: 0x{status:08x}"));
            }
            let status = unsafe { PdhCollectQueryData(query) };
            if status != 0 {
                return Err(format!("PDH baseline unavailable: 0x{status:08x}"));
            }
            result.baseline = Instant::now();
            Ok(result)
        }
        fn values(&mut self) -> Result<Vec<(String, f64)>, String> {
            if self.baseline.elapsed() < Duration::from_millis(200) {
                return Err(
                    "PDH warming up: two samples separated by at least 200 ms are required".into(),
                );
            }
            let status = unsafe { PdhCollectQueryData(self.query) };
            if status != 0 {
                return Err(format!("PdhCollectQueryData: 0x{status:08x}"));
            }
            if self.scalar {
                let mut value = PDH_FMT_COUNTERVALUE::default();
                let status = unsafe {
                    PdhGetFormattedCounterValue(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        ptr::null_mut(),
                        &mut value,
                    )
                };
                if status != 0 || !matches!(value.CStatus, 0 | 1) {
                    return Err(format!(
                        "PDH scalar unavailable: status=0x{status:08x}, CStatus=0x{:08x}",
                        value.CStatus
                    ));
                }
                let number = formatted_value(&value).ok_or("PDH scalar is not finite")?;
                return Ok(vec![("_Total".into(), number)]);
            }
            // Instance sets can grow between sizing and retrieval. Re-probe with size=0.
            for _ in 0..4 {
                let mut bytes = 0;
                let mut count = 0;
                let status = unsafe {
                    PdhGetFormattedCounterArrayW(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        ptr::null_mut(),
                    )
                };
                if status != PDH_MORE_DATA {
                    return Err(format!("PDH array sizing unavailable: 0x{status:08x}"));
                }
                let mut buffer = Buffer::new(bytes as usize).map_err(|e| e.to_string())?;
                bytes = buffer.len() as u32;
                let items = buffer.ptr::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                let status = unsafe {
                    PdhGetFormattedCounterArrayW(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        items,
                    )
                };
                if status == PDH_MORE_DATA {
                    continue;
                }
                if status != 0 {
                    return Err(format!("PDH array unavailable: 0x{status:08x}"));
                }
                let required = (count as usize)
                    .checked_mul(size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>())
                    .ok_or("PDH count overflow")?;
                buffer.offset(items, required).map_err(|e| e.to_string())?;
                let mut values = Vec::new();
                for i in 0..count as usize {
                    let item = buffer
                        .read(unsafe { items.add(i) })
                        .map_err(|e| e.to_string())?;
                    if let Some(value) = formatted_value(&item.FmtValue) {
                        values.push((buffer.wide(item.szName).map_err(|e| e.to_string())?, value));
                    }
                }
                if values.is_empty() {
                    return Err(
                        "PDH has no valid counter instances (CStatus must be 0 or 1)".into(),
                    );
                }
                return Ok(values);
            }
            Err("PDH instance set kept changing during retrieval".into())
        }
    }
    impl Drop for Counter {
        fn drop(&mut self) {
            if !self.query.is_null() {
                unsafe { PdhCloseQuery(self.query) };
            }
        }
    }
    pub(super) struct Sampler {
        cpu: Option<Counter>,
        gpu: Option<Counter>,
        disk: Option<Counter>,
        diagnostics: Vec<String>,
    }
    impl Sampler {
        pub(super) fn new() -> Self {
            let mut diagnostics = Vec::new();
            let mut open = |name, path, scalar| match Counter::new(path, scalar) {
                Ok(counter) => Some(counter),
                Err(error) => {
                    diagnostics.push(format!("{name}: {error}"));
                    None
                }
            };
            let cpu = open("CPU usage", r"\Processor(_Total)\% Processor Time", true);
            let gpu = open("GPU", r"\GPU Engine(*)\Utilization Percentage", false);
            let disk = open("Disk", r"\PhysicalDisk(*)\% Idle Time", false);
            Self {
                cpu,
                gpu,
                disk,
                diagnostics,
            }
        }
        pub(super) fn sample(&mut self, disk_name: &str) -> HardwareSnapshot {
            let mut result = HardwareSnapshot {
                diagnostics: self.diagnostics.clone(),
                ..HardwareSnapshot::default()
            };
            match cpu_frequencies() {
                Ok(values) => {
                    let positive: Vec<_> = values.iter().copied().filter(|v| *v > 0).collect();
                    if !positive.is_empty() {
                        result.cpu_frequency_mhz = Some(
                            positive.iter().map(|v| f64::from(*v)).sum::<f64>()
                                / positive.len() as f64,
                        );
                    } else {
                        result
                            .diagnostics
                            .push("CPU frequency: no processor reported CurrentMhz".into());
                    }
                    result.cpu_frequencies_mhz = values;
                }
                Err(error) => result.diagnostics.push(format!("CPU frequency: {error}")),
            }
            if let Some(cpu) = &mut self.cpu {
                match cpu.values() {
                    Ok(values) => {
                        result.cpu_usage_percent = values.first().map(|(_, v)| v.clamp(0.0, 100.0))
                    }
                    Err(error) => result.diagnostics.push(format!("CPU usage: {error}")),
                }
            }
            if let Some(gpu) = &mut self.gpu {
                match gpu.values() {
                    Ok(values) => {
                        result.gpu_usage_percent = gpu_usage(&values);
                        if result.gpu_usage_percent.is_none() {
                            result
                                .diagnostics
                                .push("GPU: no valid engtype instances".into());
                        }
                    }
                    Err(error) => result.diagnostics.push(format!("GPU: {error}")),
                }
            }
            if let Some(disk) = &mut self.disk {
                match disk.values() {
                    Ok(values) => {
                        result.disks = disk_values(&values);
                        result.disk_usage_percent = selected_disk(&result.disks, disk_name);
                        if !disk_name.is_empty()
                            && !result.disks.iter().any(|(n, _)| n == disk_name)
                        {
                            result.diagnostics.push(format!(
                                "Disk {disk_name:?} is unavailable; selecting _Total if present"
                            ));
                        }
                        if result.disk_usage_percent.is_none() {
                            result
                                .diagnostics
                                .push("Disk: neither selected disk nor _Total is available".into());
                        }
                    }
                    Err(error) => result.diagnostics.push(format!("Disk: {error}")),
                }
            }
            result
        }
    }
    fn cpu_frequencies() -> Result<Vec<u32>, String> {
        let count = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) } as usize;
        if count == 0 || count > 65536 {
            return Err("Invalid active processor count".into());
        }
        let mut values = vec![PROCESSOR_POWER_INFORMATION::default(); count];
        let bytes = u32::try_from(count * size_of::<PROCESSOR_POWER_INFORMATION>())
            .map_err(|_| "Processor buffer overflow")?;
        let status = unsafe {
            CallNtPowerInformation(
                ProcessorInformation,
                ptr::null(),
                0,
                values.as_mut_ptr().cast(),
                bytes,
            )
        };
        if status < 0 {
            return Err(format!(
                "CallNtPowerInformation NTSTATUS=0x{:08x}",
                status as u32
            ));
        }
        Ok(values.iter().map(|value| value.CurrentMhz).collect())
    }

    fn socket(
        buffer: &Buffer,
        address: SOCKET_ADDRESS,
        prefix: Option<u8>,
    ) -> io::Result<Option<String>> {
        if address.lpSockaddr.is_null() {
            return Ok(None);
        }
        let bytes = usize::try_from(address.iSockaddrLength)
            .map_err(|_| invalid("Negative sockaddr length"))?;
        let offset = buffer.offset(address.lpSockaddr.cast::<u8>(), bytes)?;
        if bytes < 2 {
            return Err(invalid("Truncated sockaddr"));
        }
        let data = unsafe {
            std::slice::from_raw_parts(buffer.words.as_ptr().cast::<u8>().add(offset), bytes)
        };
        let family = u16::from_ne_bytes([data[0], data[1]]);
        let value = if family == AF_INET {
            if bytes < 16 {
                return Err(invalid("Truncated IPv4 sockaddr"));
            }
            let ip = Ipv4Addr::new(data[4], data[5], data[6], data[7]);
            if prefix.is_some_and(|p| p > 32) {
                return Err(invalid("Invalid IPv4 prefix length"));
            }
            ip.to_string()
        } else if family == AF_INET6 {
            if bytes < 28 {
                return Err(invalid("Truncated IPv6 sockaddr"));
            }
            let ip = Ipv6Addr::from(
                <[u8; 16]>::try_from(&data[8..24]).map_err(|_| invalid("Invalid IPv6 bytes"))?,
            );
            if prefix.is_some_and(|p| p > 128) {
                return Err(invalid("Invalid IPv6 prefix length"));
            }
            let scope = u32::from_ne_bytes(
                data[24..28]
                    .try_into()
                    .map_err(|_| invalid("Invalid IPv6 scope"))?,
            );
            if scope == 0 {
                ip.to_string()
            } else {
                format!("{ip}%{scope}")
            }
        } else {
            return Ok(None);
        };
        Ok(Some(if let Some(prefix) = prefix {
            format!("{value}/{prefix}")
        } else {
            value
        }))
    }
    fn visit<T>(seen: &mut HashSet<usize>, node: *const T) -> io::Result<()> {
        if seen.len() >= MAX_NODES || !seen.insert(node as usize) {
            Err(invalid(
                "Native address list contains a cycle or exceeds its limit",
            ))
        } else {
            Ok(())
        }
    }
    fn address_node<T: Copy>(buffer: &Buffer, p: *const T) -> io::Result<T> {
        let length = buffer.read(p.cast::<u32>())? as usize;
        if length < size_of::<T>() {
            return Err(invalid("Truncated native address node"));
        }
        buffer.offset(p, length)?;
        buffer.read(p)
    }
    fn unicast(
        buffer: &Buffer,
        mut p: *mut IP_ADAPTER_UNICAST_ADDRESS_LH,
    ) -> io::Result<Vec<String>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        while !p.is_null() {
            visit(&mut seen, p)?;
            let node = address_node(buffer, p)?;
            if let Some(ip) = socket(buffer, node.Address, Some(node.OnLinkPrefixLength))? {
                if !result.contains(&ip) {
                    result.push(ip);
                }
            }
            p = node.Next;
        }
        Ok(result)
    }
    fn dns(
        buffer: &Buffer,
        mut p: *mut IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
    ) -> io::Result<Vec<String>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        while !p.is_null() {
            visit(&mut seen, p)?;
            let node = address_node(buffer, p)?;
            if let Some(ip) = socket(buffer, node.Address, None)? {
                if !result.contains(&ip) {
                    result.push(ip);
                }
            }
            p = node.Next;
        }
        Ok(result)
    }
    fn gateways(
        buffer: &Buffer,
        mut p: *mut IP_ADAPTER_GATEWAY_ADDRESS_LH,
    ) -> io::Result<Vec<String>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        while !p.is_null() {
            visit(&mut seen, p)?;
            let node = address_node(buffer, p)?;
            if let Some(ip) = socket(buffer, node.Address, None)? {
                if !result.contains(&ip) {
                    result.push(ip);
                }
            }
            p = node.Next;
        }
        Ok(result)
    }
    pub(super) fn network_details() -> io::Result<Vec<AdapterDetail>> {
        let mut bytes = 15 * 1024;
        for _ in 0..5 {
            let mut buffer = Buffer::new(bytes as usize)?;
            bytes = buffer.len() as u32;
            let first = buffer.ptr::<IP_ADAPTER_ADDRESSES_LH>();
            let status = unsafe {
                GetAdaptersAddresses(
                    u32::from(AF_UNSPEC),
                    GAA_FLAG_INCLUDE_PREFIX
                        | GAA_FLAG_INCLUDE_GATEWAYS
                        | GAA_FLAG_SKIP_ANYCAST
                        | GAA_FLAG_SKIP_MULTICAST,
                    ptr::null(),
                    first,
                    &mut bytes,
                )
            };
            if status == ERROR_BUFFER_OVERFLOW {
                continue;
            }
            if status == ERROR_NO_DATA {
                return Ok(Vec::new());
            }
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status as i32));
            }
            return decode_adapters(&buffer, first);
        }
        Err(io::Error::other(
            "Adapter address list kept changing during retrieval",
        ))
    }
    fn decode_adapters(
        buffer: &Buffer,
        mut p: *mut IP_ADAPTER_ADDRESSES_LH,
    ) -> io::Result<Vec<AdapterDetail>> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut ids = HashSet::new();
        while !p.is_null() {
            visit(&mut seen, p)?;
            let adapter = buffer.read(p)?;
            if unsafe { adapter.Anonymous1.Anonymous.Length }
                < (size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32)
            {
                return Err(invalid(
                    "Adapter structure predates supported Windows 10 layout",
                ));
            }
            let id = unsafe { adapter.Luid.Value };
            let mut row = MIB_IF_ROW2 {
                InterfaceLuid: adapter.Luid,
                ..MIB_IF_ROW2::default()
            };
            let has_row = unsafe { GetIfEntry2(&mut row) } == 0;
            // Filter interfaces must never acquire a copied physical adapter's IP identity.
            let filter = has_row && filter_flags(row.InterfaceAndOperStatusFlags._bitfield);
            if id != 0 && !filter && ids.insert(id) {
                let mac_length =
                    (adapter.PhysicalAddressLength as usize).min(adapter.PhysicalAddress.len());
                result.push(AdapterDetail {
                    id,
                    interface_index: unsafe { adapter.Anonymous1.Anonymous.IfIndex },
                    alias: buffer.wide(adapter.FriendlyName)?,
                    description: buffer.wide(adapter.Description)?,
                    adapter_name: buffer.ansi(adapter.AdapterName)?,
                    mac: adapter.PhysicalAddress[..mac_length]
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join("-"),
                    addresses: unicast(buffer, adapter.FirstUnicastAddress)?,
                    dns: dns(buffer, adapter.FirstDnsServerAddress)?,
                    gateways: gateways(buffer, adapter.FirstGatewayAddress)?,
                    mtu: adapter.Mtu,
                    link_speed_bps: adapter.TransmitLinkSpeed.max(adapter.ReceiveLinkSpeed),
                    physical: if has_row {
                        row.InterfaceAndOperStatusFlags._bitfield & 0x01 != 0
                    } else {
                        matches!(adapter.IfType, 6 | 71)
                    },
                    operational: adapter.OperStatus == IfOperStatusUp,
                });
            }
            p = adapter.Next;
        }
        Ok(result)
    }

    // MIB_IF_ROW2: HardwareInterface bit0, FilterInterface bit1, ConnectorPresent bit2.
    fn filter_flags(flags: u8) -> bool {
        flags & 0x02 != 0
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_buffer_checks_alignment_overflow_and_unterminated_strings() {
            let mut buffer = Buffer::new(64).unwrap();
            let base = buffer.ptr::<u8>();
            assert!(buffer.read::<u64>(base.wrapping_add(1).cast()).is_ok());
            assert!(buffer.wide(base.wrapping_add(1).cast()).is_err());
            assert!(buffer.read::<u64>(base.wrapping_add(64).cast()).is_err());
            assert!(buffer.offset(base, usize::MAX).is_err());
            buffer.words.fill(u64::MAX);
            assert!(buffer.wide(base.cast()).is_err());
            assert!(Buffer::new(MAX_BUFFER + 1).is_err());
        }
        #[test]
        fn sockaddr_decodes_v4_prefix_and_v6_scope_without_winsock_startup() {
            let mut buffer = Buffer::new(64).unwrap();
            let base = buffer.ptr::<u8>();
            unsafe {
                ptr::copy_nonoverlapping(
                    [2u8, 0, 0, 0, 192, 168, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0].as_ptr(),
                    base,
                    16,
                );
            }
            let address = SOCKET_ADDRESS {
                lpSockaddr: base.cast(),
                iSockaddrLength: 16,
            };
            assert_eq!(
                socket(&buffer, address, Some(24)).unwrap(),
                Some("192.168.2.3/24".into())
            );
            assert!(socket(&buffer, address, Some(33)).is_err());
            let mut data = [0u8; 28];
            data[0] = 23;
            data[8] = 0xfe;
            data[9] = 0x80;
            data[23] = 1;
            data[24] = 12;
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), base, 28);
            }
            assert_eq!(
                socket(
                    &buffer,
                    SOCKET_ADDRESS {
                        lpSockaddr: base.cast(),
                        iSockaddrLength: 28
                    },
                    Some(64)
                )
                .unwrap(),
                Some("fe80::1%12/64".into())
            );
        }
        #[test]
        fn native_address_cycles_are_reported() {
            let mut seen = HashSet::new();
            let value = 0u64;
            assert!(visit(&mut seen, &value).is_ok());
            assert!(visit(&mut seen, &value).is_err());
        }
        #[test]
        fn filter_bit_is_distinct_from_hardware_connector_bit() {
            assert!(filter_flags(0x02));
            assert!(!filter_flags(0x05));
        }
        #[test]
        fn native_short_address_nodes_are_rejected() {
            let mut buffer = Buffer::new(128).unwrap();
            let p = buffer.ptr::<IP_ADAPTER_UNICAST_ADDRESS_LH>();
            unsafe {
                p.write(IP_ADAPTER_UNICAST_ADDRESS_LH::default());
            }
            assert!(unicast(&buffer, p).is_err());
        }
        #[test]
        fn pdh_only_accepts_valid_and_new_status_and_finite_values() {
            for status in [0, 1, 2, 0xc0000bba] {
                let value = PDH_FMT_COUNTERVALUE {
                    CStatus: status,
                    Anonymous: PDH_FMT_COUNTERVALUE_0 { doubleValue: 42.0 },
                };
                assert_eq!(
                    formatted_value(&value),
                    matches!(status, 0 | 1).then_some(42.0)
                );
            }
            assert_eq!(
                formatted_value(&PDH_FMT_COUNTERVALUE {
                    CStatus: 0,
                    Anonymous: PDH_FMT_COUNTERVALUE_0 {
                        doubleValue: f64::NAN
                    }
                }),
                None
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpu_uses_busiest_engine_group_instead_of_total_sum() {
        let values = vec![
            ("pid_1_engtype_3D".into(), 30.0),
            ("pid_2_engtype_3D".into(), 20.0),
            ("pid_1_engtype_Copy".into(), 40.0),
        ];
        assert_eq!(gpu_usage(&values), Some(50.0));
    }
    #[test]
    fn gpu_rejects_missing_negative_and_nan_data() {
        assert_eq!(gpu_usage(&[]), None);
        assert_eq!(gpu_usage(&[("bad".into(), 30.0)]), None);
        assert_eq!(gpu_usage(&[("engtype_3D".into(), f64::NAN)]), None);
        assert_eq!(gpu_usage(&[("engtype_3D".into(), -1.0)]), None);
    }
    #[test]
    fn gpu_clamps_after_group_sum() {
        assert_eq!(
            gpu_usage(&[("engtype_3D".into(), 80.0), ("engtype_3D#1".into(), 90.0)]),
            Some(100.0)
        );
    }
    #[test]
    fn disk_idle_is_clamped_before_subtraction() {
        assert_eq!(
            disk_values(&[
                ("disk".into(), 400.0),
                ("bad".into(), f64::NAN),
                ("busy".into(), -5.0)
            ]),
            vec![("disk".into(), 0.0), ("busy".into(), 100.0)]
        );
    }
    #[test]
    fn disk_selection_uses_named_disk_then_total() {
        let disks = vec![("0 C:".into(), 10.0), ("_Total".into(), 20.0)];
        assert_eq!(selected_disk(&disks, "0 C:"), Some(10.0));
        assert_eq!(selected_disk(&disks, "missing"), Some(20.0));
        assert_eq!(selected_disk(&[], ""), None);
    }
}
