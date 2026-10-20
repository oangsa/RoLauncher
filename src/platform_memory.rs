// Included inside the Windows platform implementation; all access is read-only.
pub struct MemoryLogReader {
    handle: Handle,
    cursor: usize,
    newest: chrono::DateTime<chrono::Utc>,
}
unsafe impl Send for MemoryLogReader {}

impl MemoryLogReader {
    pub fn new(identity: &ProcessIdentity) -> Option<Self> {
        let handle = process_handle(
            identity.pid,
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | SYNCHRONIZE,
        )
        .ok()?;
        if process_time(handle.0).ok()? != identity.creation_time || !valid_player_image(handle.0) {
            return None;
        }
        Some(Self {
            handle,
            cursor: 0,
            newest: chrono::Utc::now() - chrono::Duration::seconds(15),
        })
    }
    /// Bounded supplemental scan. Access denial or absent log buffers is not a disconnect.
    pub fn read(
        &mut self,
        disk_time: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Option<(chrono::DateTime<chrono::Utc>, crate::logs::Signal)> {
        use windows_sys::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Memory::*};
        let now = chrono::Utc::now();
        let floor = self
            .newest
            .max(disk_time.unwrap_or(self.newest))
            .max(now - chrono::Duration::seconds(15));
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(10);
        let mut buffer = zeroize::Zeroizing::new(vec![0u8; 64 * 1024]);
        let mut budget = 2 * 1024 * 1024;
        let mut latest = None;
        for _ in 0..256 {
            if budget == 0 || std::time::Instant::now() >= deadline {
                break;
            }
            let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe {
                VirtualQueryEx(
                    self.handle.0,
                    self.cursor as _,
                    &mut info,
                    std::mem::size_of_val(&info),
                )
            } == 0
            {
                self.cursor = 0;
                break;
            }
            let Some(end) = (info.BaseAddress as usize)
                .checked_add(info.RegionSize)
                .filter(|end| *end > self.cursor)
            else {
                self.cursor = 0;
                break;
            };
            if info.State != MEM_COMMIT
                || info.Type != MEM_PRIVATE
                || info.Protect & (PAGE_GUARD | PAGE_NOACCESS | PAGE_NOCACHE) != 0
                || !matches!(
                    info.Protect & 0xff,
                    PAGE_READWRITE | PAGE_READONLY | PAGE_WRITECOPY
                )
            {
                self.cursor = end;
                continue;
            }
            let length = buffer.len().min(end - self.cursor).min(budget);
            let mut read = 0;
            let ok = unsafe {
                ReadProcessMemory(
                    self.handle.0,
                    self.cursor as _,
                    buffer.as_mut_ptr() as _,
                    length,
                    &mut read,
                )
            } != 0;
            self.cursor += if self.cursor + length < end && length > 2048 {
                length - 2048
            } else {
                length
            };
            budget -= length;
            if ok
                && let Some(event) = memory_log_event(&buffer[..read], floor, now)
                && latest.as_ref().is_none_or(|(time, _)| event.0 > *time)
            {
                latest = Some(event);
            }
            use zeroize::Zeroize;
            buffer.as_mut_slice().zeroize();
        }
        if let Some((time, _)) = latest {
            self.newest = time;
        }
        latest
    }
}

fn memory_log_event(
    bytes: &[u8],
    floor: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<(chrono::DateTime<chrono::Utc>, crate::logs::Signal)> {
    bytes
        .split_inclusive(|b| *b == 0 || *b == b'\n')
        .filter(|line| line.last().is_some_and(|b| *b == 0 || *b == b'\n'))
        .filter(|line| (25..=2048).contains(&line.len()))
        .filter_map(|line| {
            let line = std::str::from_utf8(line).ok()?;
            let time = crate::logs::timestamp(line)?;
            if time <= floor || time > now {
                return None;
            }
            let signal = crate::logs::parse_line(line)?;
            Some((time, signal))
        })
        .max_by_key(|(time, _)| *time)
}

#[cfg(test)]
mod memory_tests {
    use super::*;
    #[test]
    fn memory_logs_require_fresh_timestamp_authoritative_channel_and_newer_than_disk() {
        let now = chrono::Utc::now();
        let time = now - chrono::Duration::seconds(1);
        let line = format!(
            "{},1,1,7 [FLog::Network] Sending disconnect with reason: 273\n",
            time.to_rfc3339()
        );
        assert_eq!(
            memory_log_event(line.as_bytes(), now - chrono::Duration::seconds(15), now)
                .unwrap()
                .1,
            crate::logs::Signal::SessionLost(Some(273))
        );
        assert!(memory_log_event(line.as_bytes(), now, now).is_none());
        assert!(memory_log_event(line.as_bytes(), time, now).is_none());
        assert!(
            memory_log_event(
                b"[FLog::Network] Sending disconnect with reason: 273",
                time,
                now
            )
            .is_none()
        );
        let spoof = line.replace("[FLog::Network]", "[FLog::Output] [FLog::Network]");
        assert!(
            memory_log_event(spoof.as_bytes(), now - chrono::Duration::seconds(15), now).is_none()
        );
    }
    #[test]
    fn read_only_scan_reads_a_local_buffer_without_changing_it() {
        let now = chrono::Utc::now();
        let data = format!(
            "{},1,1,7 [FLog::Network] Sending disconnect with reason: 273\n",
            (now - chrono::Duration::seconds(1)).to_rfc3339()
        );
        let original = data.clone();
        let handle = process_handle(
            unsafe { GetCurrentProcessId() },
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | SYNCHRONIZE,
        )
        .unwrap();
        let mut reader = MemoryLogReader {
            handle,
            cursor: data.as_ptr() as usize,
            newest: now - chrono::Duration::seconds(15),
        };
        assert_eq!(
            reader.read(None).unwrap().1,
            crate::logs::Signal::SessionLost(Some(273))
        );
        assert_eq!(data, original);
    }
}
