//! Cache misses from the CPU's counters (Linux `perf_event_open`, user space only), when the
//! machine allows it. In VMs without a virtual PMU every counter is unavailable, and the harness
//! reports "n/a". The rest of the crate never depends on this.

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod imp {
    extern "C" {
        fn syscall(n: i64, ...) -> i64;
        fn ioctl(fd: i32, req: u64, ...) -> i32;
        fn read(fd: i32, buf: *mut u8, n: usize) -> isize;
        fn close(fd: i32) -> i32;
    }
    pub struct Counter(i32);
    impl Counter {
        /// Last-level cache misses (PERF_COUNT_HW_CACHE_MISSES), user space only.
        pub fn cache_misses() -> Option<Counter> {
            let mut a = [0u64; 16];
            a[0] = 128u64 << 32; // type HARDWARE (0), size 128
            a[1] = 3; // CACHE_MISSES
            a[5] = 1 | (1 << 5) | (1 << 6); // disabled, exclude_kernel, exclude_hv
            // SAFETY: a valid perf_event_attr buffer, pid 0 (self), any cpu, no group.
            let fd = unsafe { syscall(298, a.as_ptr(), 0i32, -1i32, -1i32, 0u64) } as i32;
            if fd < 0 { None } else { Some(Counter(fd)) }
        }
        pub fn start(&self) { unsafe { ioctl(self.0, 0x2403); ioctl(self.0, 0x2400); } }
        pub fn stop(&self) -> u64 {
            let mut b = [0u8; 8];
            unsafe { ioctl(self.0, 0x2401); read(self.0, b.as_mut_ptr(), 8); }
            u64::from_le_bytes(b)
        }
    }
    impl Drop for Counter { fn drop(&mut self) { unsafe { close(self.0); } } }
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
mod imp {
    pub struct Counter;
    impl Counter {
        pub fn cache_misses() -> Option<Counter> { None }
        pub fn start(&self) {}
        pub fn stop(&self) -> u64 { 0 }
    }
}

pub use imp::Counter;
